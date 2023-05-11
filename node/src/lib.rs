#[cfg(target_os = "macos")]
#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

use at_rule_parser::{AtRule, CustomAtRuleConfig, CustomAtRuleParser};
use lightningcss::bundler::{BundleErrorKind, Bundler, FileProvider, SourceProvider};
use lightningcss::css_modules::{CssModuleExports, CssModuleReferences, PatternParseError};
use lightningcss::dependencies::{Dependency, DependencyOptions};
use lightningcss::error::{Error, ErrorLocation, MinifyErrorKind, ParserError, PrinterErrorKind};
use lightningcss::stylesheet::{
  MinifyOptions, ParserOptions, PrinterOptions, PseudoClasses, StyleAttribute, StyleSheet,
};
use lightningcss::targets::Browsers;
use lightningcss::visitor::Visit;
use parcel_sourcemap::SourceMap;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock};

use transformer::JsVisitor;

mod at_rule_parser;
#[cfg(not(target_arch = "wasm32"))]
mod threadsafe_function;
mod transformer;

use napi::{CallContext, Env, JsObject, JsUnknown};
use napi_derive::{js_function, module_exports};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TransformResult<'i> {
  #[serde(with = "serde_bytes")]
  code: Vec<u8>,
  #[serde(with = "serde_bytes")]
  map: Option<Vec<u8>>,
  exports: Option<CssModuleExports>,
  references: Option<CssModuleReferences>,
  dependencies: Option<Vec<Dependency>>,
  warnings: Vec<Warning<'i>>,
}

impl<'i> TransformResult<'i> {
  fn into_js(self, env: Env) -> napi::Result<JsUnknown> {
    // Manually construct buffers so we avoid a copy and work around
    // https://github.com/napi-rs/napi-rs/issues/1124.
    let mut obj = env.create_object()?;
    let buf = env.create_buffer_with_data(self.code)?;
    obj.set_named_property("code", buf.into_raw())?;
    obj.set_named_property(
      "map",
      if let Some(map) = self.map {
        let buf = env.create_buffer_with_data(map)?;
        buf.into_raw().into_unknown()
      } else {
        env.get_null()?.into_unknown()
      },
    )?;
    obj.set_named_property("exports", env.to_js_value(&self.exports)?)?;
    obj.set_named_property("references", env.to_js_value(&self.references)?)?;
    obj.set_named_property("dependencies", env.to_js_value(&self.dependencies)?)?;
    obj.set_named_property("warnings", env.to_js_value(&self.warnings)?)?;
    Ok(obj.into_unknown())
  }
}

#[js_function(1)]
fn transform(ctx: CallContext) -> napi::Result<JsUnknown> {
  use transformer::JsVisitor;

  let opts = ctx.get::<JsObject>(0)?;
  let mut visitor = if let Ok(visitor) = opts.get_named_property::<JsObject>("visitor") {
    Some(JsVisitor::new(*ctx.env, visitor))
  } else {
    None
  };

  let config: Config = ctx.env.from_js_value(opts)?;
  let code = unsafe { std::str::from_utf8_unchecked(&config.code) };
  let res = compile(code, &config, &mut visitor);

  match res {
    Ok(res) => res.into_js(*ctx.env),
    Err(err) => err.throw(*ctx.env, Some(code)),
  }
}

#[js_function(1)]
fn transform_style_attribute(ctx: CallContext) -> napi::Result<JsUnknown> {
  use transformer::JsVisitor;

  let opts = ctx.get::<JsObject>(0)?;
  let mut visitor = if let Ok(visitor) = opts.get_named_property::<JsObject>("visitor") {
    Some(JsVisitor::new(*ctx.env, visitor))
  } else {
    None
  };

  let config: AttrConfig = ctx.env.from_js_value(opts)?;
  let code = unsafe { std::str::from_utf8_unchecked(&config.code) };
  let res = compile_attr(code, &config, &mut visitor);

  match res {
    Ok(res) => res.into_js(ctx),
    Err(err) => err.throw(*ctx.env, Some(code)),
  }
}

#[cfg(not(target_arch = "wasm32"))]
mod bundle {
  use super::*;
  use crossbeam_channel::{self, Receiver, Sender};
  use napi::{Env, JsFunction, JsString, NapiRaw};
  use threadsafe_function::{ThreadSafeCallContext, ThreadsafeFunction, ThreadsafeFunctionCallMode};

  #[js_function(1)]
  pub fn bundle(ctx: CallContext) -> napi::Result<JsUnknown> {
    use transformer::JsVisitor;

    let opts = ctx.get::<JsObject>(0)?;
    let mut visitor = if let Ok(visitor) = opts.get_named_property::<JsObject>("visitor") {
      Some(JsVisitor::new(*ctx.env, visitor))
    } else {
      None
    };

    let config: BundleConfig = ctx.env.from_js_value(opts)?;
    let fs = FileProvider::new();

    // This is pretty silly, but works around a rust limitation that you cannot
    // explicitly annotate lifetime bounds on closures.
    fn annotate<'i, 'o, F>(f: F) -> F
    where
      F: FnOnce(&mut StyleSheet<'i, 'o, AtRule<'i>>) -> napi::Result<()>,
    {
      f
    }

    let res = compile_bundle(
      &fs,
      &config,
      visitor.as_mut().map(|visitor| annotate(|stylesheet| stylesheet.visit(visitor))),
    );

    match res {
      Ok(res) => res.into_js(*ctx.env),
      Err(err) => {
        let code = match &err {
          CompileError::ParseError(Error {
            loc: Some(ErrorLocation { filename, .. }),
            ..
          })
          | CompileError::PrinterError(Error {
            loc: Some(ErrorLocation { filename, .. }),
            ..
          })
          | CompileError::MinifyError(Error {
            loc: Some(ErrorLocation { filename, .. }),
            ..
          })
          | CompileError::BundleError(Error {
            loc: Some(ErrorLocation { filename, .. }),
            ..
          }) => Some(fs.read(Path::new(filename))?),
          _ => None,
        };
        err.throw(*ctx.env, code)
      }
    }
  }

  // A SourceProvider which calls JavaScript functions to resolve and read files.
  struct JsSourceProvider {
    resolve: Option<ThreadsafeFunction<ResolveMessage>>,
    read: Option<ThreadsafeFunction<ReadMessage>>,
    inputs: Mutex<Vec<*mut String>>,
  }

  unsafe impl Sync for JsSourceProvider {}
  unsafe impl Send for JsSourceProvider {}

  // Allocate a single channel per thread to communicate with the JS thread.
  thread_local! {
    static CHANNEL: (Sender<napi::Result<String>>, Receiver<napi::Result<String>>) = crossbeam_channel::unbounded();
  }

  impl SourceProvider for JsSourceProvider {
    type Error = napi::Error;

    fn read<'a>(&'a self, file: &Path) -> Result<&'a str, Self::Error> {
      let source = if let Some(read) = &self.read {
        CHANNEL.with(|channel| {
          let message = ReadMessage {
            file: file.to_str().unwrap().to_owned(),
            tx: channel.0.clone(),
          };

          read.call(message, ThreadsafeFunctionCallMode::Blocking);
          channel.1.recv().unwrap()
        })
      } else {
        Ok(std::fs::read_to_string(file)?)
      };

      match source {
        Ok(source) => {
          // cache the result
          let ptr = Box::into_raw(Box::new(source));
          self.inputs.lock().unwrap().push(ptr);
          // SAFETY: this is safe because the pointer is not dropped
          // until the JsSourceProvider is, and we never remove from the
          // list of pointers stored in the vector.
          Ok(unsafe { &*ptr })
        }
        Err(e) => Err(e),
      }
    }

    fn resolve(&self, specifier: &str, originating_file: &Path) -> Result<PathBuf, Self::Error> {
      if let Some(resolve) = &self.resolve {
        return CHANNEL.with(|channel| {
          let message = ResolveMessage {
            specifier: specifier.to_owned(),
            originating_file: originating_file.to_str().unwrap().to_owned(),
            tx: channel.0.clone(),
          };

          resolve.call(message, ThreadsafeFunctionCallMode::Blocking);
          let result = channel.1.recv().unwrap();
          match result {
            Ok(result) => Ok(PathBuf::from_str(&result).unwrap()),
            Err(e) => Err(e),
          }
        });
      }

      Ok(originating_file.with_file_name(specifier))
    }
  }

  struct ResolveMessage {
    specifier: String,
    originating_file: String,
    tx: Sender<napi::Result<String>>,
  }

  struct ReadMessage {
    file: String,
    tx: Sender<napi::Result<String>>,
  }

  struct VisitMessage {
    stylesheet: &'static mut StyleSheet<'static, 'static, AtRule<'static>>,
    tx: Sender<napi::Result<String>>,
  }

  fn await_promise(env: Env, result: JsUnknown, tx: Sender<napi::Result<String>>) -> napi::Result<()> {
    // If the result is a promise, wait for it to resolve, and send the result to the channel.
    // Otherwise, send the result immediately.
    if result.is_promise()? {
      let result: JsObject = result.try_into()?;
      let then: JsFunction = result.get_named_property("then")?;
      let tx2 = tx.clone();
      let cb = env.create_function_from_closure("callback", move |ctx| {
        let res = ctx.get::<JsString>(0)?.into_utf8()?;
        let s = res.into_owned()?;
        tx.send(Ok(s)).unwrap();
        ctx.env.get_undefined()
      })?;
      let eb = env.create_function_from_closure("error_callback", move |ctx| {
        // TODO: need a way to convert a JsUnknown to an Error
        tx2.send(Err(napi::Error::from_reason("Promise rejected"))).unwrap();
        ctx.env.get_undefined()
      })?;
      then.call(Some(&result), &[cb, eb])?;
    } else {
      let result: JsString = result.try_into()?;
      let utf8 = result.into_utf8()?;
      let s = utf8.into_owned()?;
      tx.send(Ok(s)).unwrap();
    }

    Ok(())
  }

  fn resolve_on_js_thread(ctx: ThreadSafeCallContext<ResolveMessage>) -> napi::Result<()> {
    let specifier = ctx.env.create_string(&ctx.value.specifier)?;
    let originating_file = ctx.env.create_string(&ctx.value.originating_file)?;
    let result = ctx.callback.unwrap().call(None, &[specifier, originating_file])?;
    await_promise(ctx.env, result, ctx.value.tx)
  }

  fn handle_error(tx: Sender<napi::Result<String>>, res: napi::Result<()>) -> napi::Result<()> {
    match res {
      Ok(_) => Ok(()),
      Err(e) => {
        tx.send(Err(e)).expect("send error");
        Ok(())
      }
    }
  }

  fn resolve_on_js_thread_wrapper(ctx: ThreadSafeCallContext<ResolveMessage>) -> napi::Result<()> {
    let tx = ctx.value.tx.clone();
    handle_error(tx, resolve_on_js_thread(ctx))
  }

  fn read_on_js_thread(ctx: ThreadSafeCallContext<ReadMessage>) -> napi::Result<()> {
    let file = ctx.env.create_string(&ctx.value.file)?;
    let result = ctx.callback.unwrap().call(None, &[file])?;
    await_promise(ctx.env, result, ctx.value.tx)
  }

  fn read_on_js_thread_wrapper(ctx: ThreadSafeCallContext<ReadMessage>) -> napi::Result<()> {
    let tx = ctx.value.tx.clone();
    handle_error(tx, read_on_js_thread(ctx))
  }

  #[js_function(1)]
  pub fn bundle_async(ctx: CallContext) -> napi::Result<JsObject> {
    use transformer::JsVisitor;

    let opts = ctx.get::<JsObject>(0)?;
    let visitor = if let Ok(visitor) = opts.get_named_property::<JsObject>("visitor") {
      let visitor = JsVisitor::new(*ctx.env, visitor);
      Some(visitor)
    } else {
      None
    };

    let config: BundleConfig = ctx.env.from_js_value(&opts)?;

    if let Ok(resolver) = opts.get_named_property::<JsObject>("resolver") {
      let read = if resolver.has_named_property("read")? {
        let read = resolver.get_named_property::<JsFunction>("read")?;
        Some(ThreadsafeFunction::create(
          ctx.env.raw(),
          unsafe { read.raw() },
          0,
          read_on_js_thread_wrapper,
        )?)
      } else {
        None
      };

      let resolve = if resolver.has_named_property("resolve")? {
        let resolve = resolver.get_named_property::<JsFunction>("resolve")?;
        Some(ThreadsafeFunction::create(
          ctx.env.raw(),
          unsafe { resolve.raw() },
          0,
          resolve_on_js_thread_wrapper,
        )?)
      } else {
        None
      };

      let provider = JsSourceProvider {
        resolve,
        read,
        inputs: Mutex::new(Vec::new()),
      };

      run_bundle_task(provider, config, visitor, *ctx.env)
    } else {
      let provider = FileProvider::new();
      run_bundle_task(provider, config, visitor, *ctx.env)
    }
  }

  // Runs bundling on a background thread managed by rayon. This is similar to AsyncTask from napi-rs, however,
  // because we call back into the JS thread, which might call other tasks in the node threadpool (e.g. fs.readFile),
  // we may end up deadlocking if the number of rayon threads exceeds node's threadpool size. Therefore, we must
  // run bundling from a thread not managed by Node.
  fn run_bundle_task<P: 'static + SourceProvider>(
    provider: P,
    config: BundleConfig,
    visitor: Option<JsVisitor>,
    env: Env,
  ) -> napi::Result<JsObject> {
    let (deferred, promise) = env.create_deferred()?;

    let tsfn = if let Some(mut visitor) = visitor {
      Some(ThreadsafeFunction::create(
        env.raw(),
        std::ptr::null_mut(),
        0,
        move |ctx: ThreadSafeCallContext<VisitMessage>| {
          if let Err(err) = ctx.value.stylesheet.visit(&mut visitor) {
            ctx.value.tx.send(Err(err.clone())).expect("send error");
            return Ok(());
          }
          ctx.value.tx.send(Ok(Default::default())).expect("send error");
          Ok(())
        },
      )?)
    } else {
      None
    };

    // Run bundling task in rayon threadpool.
    rayon::spawn(move || {
      match compile_bundle(
        unsafe { std::mem::transmute::<&'_ P, &'static P>(&provider) },
        &config,
        tsfn.map(move |tsfn| {
          move |stylesheet: &mut StyleSheet<AtRule>| {
            CHANNEL.with(|channel| {
              let message = VisitMessage {
                // SAFETY: we immediately lock the thread until we get a response,
                // so stylesheet cannot be dropped in that time.
                stylesheet: unsafe {
                  std::mem::transmute::<
                    &'_ mut StyleSheet<'_, '_, AtRule>,
                    &'static mut StyleSheet<'static, 'static, AtRule>,
                  >(stylesheet)
                },
                tx: channel.0.clone(),
              };

              tsfn.call(message, ThreadsafeFunctionCallMode::Blocking);
              channel.1.recv().expect("recv error").map(|_| ())
            })
          }
        }),
      ) {
        Ok(v) => deferred.resolve(|env| v.into_js(env)),
        Err(err) => deferred.reject(err.into()),
      }
    });

    Ok(promise)
  }
}

#[cfg_attr(not(target_arch = "wasm32"), module_exports)]
fn init(mut exports: JsObject) -> napi::Result<()> {
  exports.create_named_method("transform", transform)?;
  exports.create_named_method("transformStyleAttribute", transform_style_attribute)?;

  #[cfg(not(target_arch = "wasm32"))]
  {
    exports.create_named_method("bundle", bundle::bundle)?;
    exports.create_named_method("bundleAsync", bundle::bundle_async)?;
  }

  Ok(())
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub unsafe fn napi_register_wasm_v1(raw_env: napi::sys::napi_env, raw_exports: napi::sys::napi_value) {
  use napi::{Env, JsObject, NapiValue};

  let env = Env::from_raw(raw_env);
  let exports = JsObject::from_raw_unchecked(raw_env, raw_exports);
  init(exports);
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn napi_wasm_malloc(size: usize) -> *mut u8 {
  use std::alloc::{alloc, Layout};
  use std::mem;

  let align = mem::align_of::<usize>();
  if let Ok(layout) = Layout::from_size_align(size, align) {
    unsafe {
      if layout.size() > 0 {
        let ptr = alloc(layout);
        if !ptr.is_null() {
          return ptr;
        }
      } else {
        return align as *mut u8;
      }
    }
  }

  std::process::abort();
}

// ---------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Config {
  pub filename: Option<String>,
  pub project_root: Option<String>,
  #[serde(with = "serde_bytes")]
  pub code: Vec<u8>,
  pub targets: Option<Browsers>,
  pub minify: Option<bool>,
  pub source_map: Option<bool>,
  pub input_source_map: Option<String>,
  pub drafts: Option<Drafts>,
  pub css_modules: Option<CssModulesOption>,
  pub analyze_dependencies: Option<AnalyzeDependenciesOption>,
  pub pseudo_classes: Option<OwnedPseudoClasses>,
  pub unused_symbols: Option<HashSet<String>>,
  pub error_recovery: Option<bool>,
  pub custom_at_rules: Option<HashMap<String, CustomAtRuleConfig>>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AnalyzeDependenciesOption {
  Bool(bool),
  Config(AnalyzeDependenciesConfig),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalyzeDependenciesConfig {
  preserve_imports: bool,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum CssModulesOption {
  Bool(bool),
  Config(CssModulesConfig),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CssModulesConfig {
  pattern: Option<String>,
  dashed_idents: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleConfig {
  pub filename: String,
  pub project_root: Option<String>,
  pub targets: Option<Browsers>,
  pub minify: Option<bool>,
  pub source_map: Option<bool>,
  pub drafts: Option<Drafts>,
  pub css_modules: Option<CssModulesOption>,
  pub analyze_dependencies: Option<AnalyzeDependenciesOption>,
  pub pseudo_classes: Option<OwnedPseudoClasses>,
  pub unused_symbols: Option<HashSet<String>>,
  pub error_recovery: Option<bool>,
  pub custom_at_rules: Option<HashMap<String, CustomAtRuleConfig>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnedPseudoClasses {
  pub hover: Option<String>,
  pub active: Option<String>,
  pub focus: Option<String>,
  pub focus_visible: Option<String>,
  pub focus_within: Option<String>,
}

impl<'a> Into<PseudoClasses<'a>> for &'a OwnedPseudoClasses {
  fn into(self) -> PseudoClasses<'a> {
    PseudoClasses {
      hover: self.hover.as_deref(),
      active: self.active.as_deref(),
      focus: self.focus.as_deref(),
      focus_visible: self.focus_visible.as_deref(),
      focus_within: self.focus_within.as_deref(),
    }
  }
}

#[derive(Serialize, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Drafts {
  #[serde(default)]
  nesting: bool,
  #[serde(default)]
  custom_media: bool,
  #[serde(default)]
  scope: bool,
}

fn compile<'i>(
  code: &'i str,
  config: &Config,
  visitor: &mut Option<JsVisitor>,
) -> Result<TransformResult<'i>, CompileError<'i, std::io::Error>> {
  let drafts = config.drafts.as_ref();
  let warnings = Some(Arc::new(RwLock::new(Vec::new())));

  let filename = config.filename.clone().unwrap_or_default();
  let project_root = config.project_root.as_ref().map(|p| p.as_ref());
  let mut source_map = if config.source_map.unwrap_or_default() {
    let mut sm = SourceMap::new(project_root.unwrap_or("/"));
    sm.add_source(&filename);
    sm.set_source_content(0, code)?;
    Some(sm)
  } else {
    None
  };

  let res = {
    let mut stylesheet = StyleSheet::parse_with(
      &code,
      ParserOptions {
        filename: filename.clone(),
        nesting: matches!(drafts, Some(d) if d.nesting),
        custom_media: matches!(drafts, Some(d) if d.custom_media),
        scope: matches!(drafts, Some(d) if d.scope),
        css_modules: if let Some(css_modules) = &config.css_modules {
          match css_modules {
            CssModulesOption::Bool(true) => Some(lightningcss::css_modules::Config::default()),
            CssModulesOption::Bool(false) => None,
            CssModulesOption::Config(c) => Some(lightningcss::css_modules::Config {
              pattern: if let Some(pattern) = c.pattern.as_ref() {
                match lightningcss::css_modules::Pattern::parse(pattern) {
                  Ok(p) => p,
                  Err(e) => return Err(CompileError::PatternError(e)),
                }
              } else {
                Default::default()
              },
              dashed_idents: c.dashed_idents.unwrap_or_default(),
            }),
          }
        } else {
          None
        },
        source_index: 0,
        error_recovery: config.error_recovery.unwrap_or_default(),
        warnings: warnings.clone(),
      },
      &mut CustomAtRuleParser {
        configs: config.custom_at_rules.clone().unwrap_or_default(),
      },
    )?;

    if let Some(visitor) = visitor.as_mut() {
      stylesheet.visit(visitor).map_err(CompileError::JsError)?;
    }

    stylesheet.minify(MinifyOptions {
      targets: config.targets,
      unused_symbols: config.unused_symbols.clone().unwrap_or_default(),
    })?;

    stylesheet.to_css(PrinterOptions {
      minify: config.minify.unwrap_or_default(),
      source_map: source_map.as_mut(),
      project_root,
      targets: config.targets,
      analyze_dependencies: if let Some(d) = &config.analyze_dependencies {
        match d {
          AnalyzeDependenciesOption::Bool(b) if *b => Some(DependencyOptions { remove_imports: true }),
          AnalyzeDependenciesOption::Config(c) => Some(DependencyOptions {
            remove_imports: !c.preserve_imports,
          }),
          _ => None,
        }
      } else {
        None
      },
      pseudo_classes: config.pseudo_classes.as_ref().map(|p| p.into()),
    })?
  };

  let map = if let Some(mut source_map) = source_map {
    if let Some(input_source_map) = &config.input_source_map {
      if let Ok(mut sm) = SourceMap::from_json("/", input_source_map) {
        let _ = source_map.extends(&mut sm);
      }
    }

    source_map.to_json(None).ok()
  } else {
    None
  };

  Ok(TransformResult {
    code: res.code.into_bytes(),
    map: map.map(|m| m.into_bytes()),
    exports: res.exports,
    references: res.references,
    dependencies: res.dependencies,
    warnings: warnings.map_or(Vec::new(), |w| {
      Arc::try_unwrap(w)
        .unwrap()
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|w| w.into())
        .collect()
    }),
  })
}

fn compile_bundle<
  'i,
  'o,
  P: SourceProvider,
  F: FnOnce(&mut StyleSheet<'i, 'o, AtRule<'i>>) -> napi::Result<()>,
>(
  fs: &'i P,
  config: &'o BundleConfig,
  visit: Option<F>,
) -> Result<TransformResult<'i>, CompileError<'i, P::Error>> {
  let project_root = config.project_root.as_ref().map(|p| p.as_ref());
  let mut source_map = if config.source_map.unwrap_or_default() {
    Some(SourceMap::new(project_root.unwrap_or("/")))
  } else {
    None
  };
  let warnings = Some(Arc::new(RwLock::new(Vec::new())));
  let res = {
    let drafts = config.drafts.as_ref();
    let parser_options = ParserOptions {
      nesting: matches!(drafts, Some(d) if d.nesting),
      custom_media: matches!(drafts, Some(d) if d.custom_media),
      scope: matches!(drafts, Some(d) if d.custom_media),
      css_modules: if let Some(css_modules) = &config.css_modules {
        match css_modules {
          CssModulesOption::Bool(true) => Some(lightningcss::css_modules::Config::default()),
          CssModulesOption::Bool(false) => None,
          CssModulesOption::Config(c) => Some(lightningcss::css_modules::Config {
            pattern: if let Some(pattern) = c.pattern.as_ref() {
              match lightningcss::css_modules::Pattern::parse(pattern) {
                Ok(p) => p,
                Err(e) => return Err(CompileError::PatternError(e)),
              }
            } else {
              Default::default()
            },
            dashed_idents: c.dashed_idents.unwrap_or_default(),
          }),
        }
      } else {
        None
      },
      error_recovery: config.error_recovery.unwrap_or_default(),
      warnings: warnings.clone(),
      filename: String::new(),
      source_index: 0,
    };

    let mut at_rule_parser = CustomAtRuleParser {
      configs: config.custom_at_rules.clone().unwrap_or_default(),
    };

    let mut bundler =
      Bundler::new_with_at_rule_parser(fs, source_map.as_mut(), parser_options, &mut at_rule_parser);
    let mut stylesheet = bundler.bundle(Path::new(&config.filename))?;

    if let Some(visit) = visit {
      visit(&mut stylesheet).map_err(CompileError::JsError)?;
    }

    stylesheet.minify(MinifyOptions {
      targets: config.targets,
      unused_symbols: config.unused_symbols.clone().unwrap_or_default(),
    })?;

    stylesheet.to_css(PrinterOptions {
      minify: config.minify.unwrap_or_default(),
      source_map: source_map.as_mut(),
      project_root,
      targets: config.targets,
      analyze_dependencies: if let Some(d) = &config.analyze_dependencies {
        match d {
          AnalyzeDependenciesOption::Bool(b) if *b => Some(DependencyOptions { remove_imports: true }),
          AnalyzeDependenciesOption::Config(c) => Some(DependencyOptions {
            remove_imports: !c.preserve_imports,
          }),
          _ => None,
        }
      } else {
        None
      },
      pseudo_classes: config.pseudo_classes.as_ref().map(|p| p.into()),
    })?
  };

  let map = if let Some(source_map) = &mut source_map {
    source_map.to_json(None).ok()
  } else {
    None
  };

  Ok(TransformResult {
    code: res.code.into_bytes(),
    map: map.map(|m| m.into_bytes()),
    exports: res.exports,
    references: res.references,
    dependencies: res.dependencies,
    warnings: warnings.map_or(Vec::new(), |w| {
      Arc::try_unwrap(w)
        .unwrap()
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|w| w.into())
        .collect()
    }),
  })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttrConfig {
  pub filename: Option<String>,
  #[serde(with = "serde_bytes")]
  pub code: Vec<u8>,
  pub targets: Option<Browsers>,
  #[serde(default)]
  pub minify: bool,
  #[serde(default)]
  pub analyze_dependencies: bool,
  #[serde(default)]
  pub error_recovery: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AttrResult<'i> {
  #[serde(with = "serde_bytes")]
  code: Vec<u8>,
  dependencies: Option<Vec<Dependency>>,
  warnings: Vec<Warning<'i>>,
}

impl<'i> AttrResult<'i> {
  fn into_js(self, ctx: CallContext) -> napi::Result<JsUnknown> {
    // Manually construct buffers so we avoid a copy and work around
    // https://github.com/napi-rs/napi-rs/issues/1124.
    let mut obj = ctx.env.create_object()?;
    let buf = ctx.env.create_buffer_with_data(self.code)?;
    obj.set_named_property("code", buf.into_raw())?;
    obj.set_named_property("dependencies", ctx.env.to_js_value(&self.dependencies)?)?;
    obj.set_named_property("warnings", ctx.env.to_js_value(&self.warnings)?)?;
    Ok(obj.into_unknown())
  }
}

fn compile_attr<'i>(
  code: &'i str,
  config: &AttrConfig,
  visitor: &mut Option<JsVisitor>,
) -> Result<AttrResult<'i>, CompileError<'i, std::io::Error>> {
  let warnings = if config.error_recovery {
    Some(Arc::new(RwLock::new(Vec::new())))
  } else {
    None
  };
  let res = {
    let filename = config.filename.clone().unwrap_or_default();
    let mut attr = StyleAttribute::parse(
      &code,
      ParserOptions {
        filename,
        error_recovery: config.error_recovery,
        warnings: warnings.clone(),
        ..ParserOptions::default()
      },
    )?;

    if let Some(visitor) = visitor.as_mut() {
      attr.visit(visitor).unwrap();
    }

    attr.minify(MinifyOptions {
      targets: config.targets,
      ..MinifyOptions::default()
    });
    attr.to_css(PrinterOptions {
      minify: config.minify,
      source_map: None,
      project_root: None,
      targets: config.targets,
      analyze_dependencies: if config.analyze_dependencies {
        Some(DependencyOptions::default())
      } else {
        None
      },
      pseudo_classes: None,
    })?
  };
  Ok(AttrResult {
    code: res.code.into_bytes(),
    dependencies: res.dependencies,
    warnings: warnings.map_or(Vec::new(), |w| {
      Arc::try_unwrap(w)
        .unwrap()
        .into_inner()
        .unwrap()
        .into_iter()
        .map(|w| w.into())
        .collect()
    }),
  })
}

enum CompileError<'i, E: std::error::Error> {
  ParseError(Error<ParserError<'i>>),
  MinifyError(Error<MinifyErrorKind>),
  PrinterError(Error<PrinterErrorKind>),
  SourceMapError(parcel_sourcemap::SourceMapError),
  BundleError(Error<BundleErrorKind<'i, E>>),
  PatternError(PatternParseError),
  JsError(napi::Error),
}

impl<'i, E: std::error::Error> std::fmt::Display for CompileError<'i, E> {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    match self {
      CompileError::ParseError(err) => err.kind.fmt(f),
      CompileError::MinifyError(err) => err.kind.fmt(f),
      CompileError::PrinterError(err) => err.kind.fmt(f),
      CompileError::BundleError(err) => err.kind.fmt(f),
      CompileError::PatternError(err) => err.fmt(f),
      CompileError::SourceMapError(err) => write!(f, "{}", err.to_string()), // TODO: switch to `fmt::Display` once parcel_sourcemap supports this
      CompileError::JsError(err) => std::fmt::Debug::fmt(&err, f),
    }
  }
}

impl<'i, E: std::error::Error> CompileError<'i, E> {
  fn throw(self, env: Env, code: Option<&str>) -> napi::Result<JsUnknown> {
    let reason = self.to_string();
    let data = match &self {
      CompileError::ParseError(Error { kind, .. }) => env.to_js_value(kind)?,
      CompileError::PrinterError(Error { kind, .. }) => env.to_js_value(kind)?,
      CompileError::MinifyError(Error { kind, .. }) => env.to_js_value(kind)?,
      CompileError::BundleError(Error { kind, .. }) => env.to_js_value(kind)?,
      _ => env.get_null()?.into_unknown(),
    };

    match self {
      CompileError::ParseError(Error { loc, .. })
      | CompileError::PrinterError(Error { loc, .. })
      | CompileError::MinifyError(Error { loc, .. })
      | CompileError::BundleError(Error { loc, .. }) => {
        // Generate an error with location information.
        let syntax_error = env.get_global()?.get_named_property::<napi::JsFunction>("SyntaxError")?;
        let reason = env.create_string_from_std(reason)?;
        let mut obj = syntax_error.new_instance(&[reason])?;
        if let Some(loc) = loc {
          let line = env.create_int32((loc.line + 1) as i32)?;
          let col = env.create_int32(loc.column as i32)?;
          let filename = env.create_string_from_std(loc.filename)?;
          obj.set_named_property("fileName", filename)?;
          if let Some(code) = code {
            let source = env.create_string(code)?;
            obj.set_named_property("source", source)?;
          }
          let mut loc = env.create_object()?;
          loc.set_named_property("line", line)?;
          loc.set_named_property("column", col)?;
          obj.set_named_property("loc", loc)?;
        }
        obj.set_named_property("data", data)?;
        env.throw(obj)?;
        Ok(env.get_undefined()?.into_unknown())
      }
      _ => Err(self.into()),
    }
  }
}

impl<'i, E: std::error::Error> From<Error<ParserError<'i>>> for CompileError<'i, E> {
  fn from(e: Error<ParserError<'i>>) -> CompileError<'i, E> {
    CompileError::ParseError(e)
  }
}

impl<'i, E: std::error::Error> From<Error<MinifyErrorKind>> for CompileError<'i, E> {
  fn from(err: Error<MinifyErrorKind>) -> CompileError<'i, E> {
    CompileError::MinifyError(err)
  }
}

impl<'i, E: std::error::Error> From<Error<PrinterErrorKind>> for CompileError<'i, E> {
  fn from(err: Error<PrinterErrorKind>) -> CompileError<'i, E> {
    CompileError::PrinterError(err)
  }
}

impl<'i, E: std::error::Error> From<parcel_sourcemap::SourceMapError> for CompileError<'i, E> {
  fn from(e: parcel_sourcemap::SourceMapError) -> CompileError<'i, E> {
    CompileError::SourceMapError(e)
  }
}

impl<'i, E: std::error::Error> From<Error<BundleErrorKind<'i, E>>> for CompileError<'i, E> {
  fn from(e: Error<BundleErrorKind<'i, E>>) -> CompileError<'i, E> {
    CompileError::BundleError(e)
  }
}

impl<'i, E: std::error::Error> From<CompileError<'i, E>> for napi::Error {
  fn from(e: CompileError<'i, E>) -> napi::Error {
    match e {
      CompileError::SourceMapError(e) => napi::Error::from_reason(e.to_string()),
      CompileError::PatternError(e) => napi::Error::from_reason(e.to_string()),
      CompileError::JsError(e) => e,
      _ => napi::Error::new(napi::Status::GenericFailure, e.to_string()),
    }
  }
}

#[derive(Serialize)]
struct Warning<'i> {
  message: String,
  #[serde(flatten)]
  data: ParserError<'i>,
  loc: Option<ErrorLocation>,
}

impl<'i> From<Error<ParserError<'i>>> for Warning<'i> {
  fn from(mut e: Error<ParserError<'i>>) -> Self {
    // Convert to 1-based line numbers.
    if let Some(loc) = &mut e.loc {
      loc.line += 1;
    }
    Warning {
      message: e.kind.to_string(),
      data: e.kind,
      loc: e.loc,
    }
  }
}
