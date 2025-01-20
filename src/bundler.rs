//! CSS bundling.
//!
//! A [Bundler](Bundler) can be used to combine a CSS file and all of its dependencies
//! into a single merged style sheet. It works together with a [SourceProvider](SourceProvider)
//! (e.g. [FileProvider](FileProvider)) to read files from the file system or another source,
//! and returns a [StyleSheet](super::stylesheet::StyleSheet) containing the rules from all
//! of the dependencies of the entry file, recursively.
//!
//! Rules are bundled following `@import` order, and wrapped in the necessary `@media`, `@supports`,
//! and `@layer` rules as appropriate to preserve the authored behavior.
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//! use lightningcss::{
//!   bundler::{Bundler, FileProvider},
//!   stylesheet::ParserOptions
//! };
//!
//! let fs = FileProvider::new();
//! let mut bundler = Bundler::new(&fs, None, ParserOptions::default());
//! let stylesheet = bundler.bundle(Path::new("style.css")).unwrap();
//! ```

use crate::{
  error::ErrorLocation,
  parser::DefaultAtRuleParser,
  properties::{
    css_modules::Specifier,
    custom::{
      CustomProperty, EnvironmentVariableName, TokenList, TokenOrValue, UnparsedProperty, UnresolvedColor,
    },
    Property,
  },
  rules::{
    layer::{LayerBlockRule, LayerName},
    Location,
  },
  traits::{AtRuleParser, ToCss},
  values::ident::DashedIdentReference,
};
use crate::{
  error::{Error, ParserError},
  media_query::MediaList,
  rules::{
    import::ImportRule,
    media::MediaRule,
    supports::{SupportsCondition, SupportsRule},
    CssRule, CssRuleList,
  },
  stylesheet::{ParserOptions, StyleSheet},
};
use dashmap::DashMap;
use parcel_sourcemap::SourceMap;
use rayon::prelude::*;
use std::{
  collections::HashSet,
  fs,
  path::{Path, PathBuf},
  sync::Mutex,
};

/// A Bundler combines a CSS file and all imported dependencies together into
/// a single merged style sheet.
pub struct Bundler<'a, 'o, 's, P, T: AtRuleParser<'a>> {
  source_map: Option<Mutex<&'s mut SourceMap>>,
  fs: &'a P,
  source_indexes: DashMap<PathBuf, u32>,
  stylesheets: Mutex<Vec<BundleStyleSheet<'a, 'o, T::AtRule>>>,
  options: ParserOptions<'o, 'a>,
  at_rule_parser: Mutex<AtRuleParserValue<'s, T>>,
}

enum AtRuleParserValue<'a, T> {
  Owned(T),
  Borrowed(&'a mut T),
}

struct BundleStyleSheet<'i, 'o, T> {
  stylesheet: Option<StyleSheet<'i, 'o, T>>,
  dependencies: Vec<Dependency>,
  css_modules_deps: Vec<u32>,
  parent_source_index: u32,
  parent_dep_index: u32,
  layer: Option<Option<LayerName<'i>>>,
  supports: Option<SupportsCondition<'i>>,
  media: MediaList<'i>,
  loc: Location,
}

#[derive(Debug, Clone)]
enum Dependency {
  File(u32),
  External(String),
}

/// The result of [SourceProvider::resolve].
#[derive(Debug)]
pub enum ResolveResult {
  /// A file path.
  File(PathBuf),
  /// An external URL.
  External(String),
}

impl From<PathBuf> for ResolveResult {
  fn from(path: PathBuf) -> Self {
    ResolveResult::File(path)
  }
}

/// A trait to provide the contents of files to a Bundler.
///
/// See [FileProvider](FileProvider) for an implementation that uses the
/// file system.
pub trait SourceProvider: Send + Sync {
  /// A custom error.
  type Error: std::error::Error + Send + Sync;

  /// Reads the contents of the given file path to a string.
  fn read<'a>(&'a self, file: &Path) -> Result<&'a str, Self::Error>;

  /// Resolves the given import specifier to a file path given the file
  /// which the import originated from.
  fn resolve(&self, specifier: &str, originating_file: &Path) -> Result<ResolveResult, Self::Error>;
}

/// Provides an implementation of [SourceProvider](SourceProvider)
/// that reads files from the file system.
pub struct FileProvider {
  inputs: Mutex<Vec<*mut String>>,
}

impl FileProvider {
  /// Creates a new FileProvider.
  pub fn new() -> FileProvider {
    FileProvider {
      inputs: Mutex::new(Vec::new()),
    }
  }
}

unsafe impl Sync for FileProvider {}
unsafe impl Send for FileProvider {}

impl SourceProvider for FileProvider {
  type Error = std::io::Error;

  fn read<'a>(&'a self, file: &Path) -> Result<&'a str, Self::Error> {
    let source = fs::read_to_string(file)?;
    let ptr = Box::into_raw(Box::new(source));
    self.inputs.lock().unwrap().push(ptr);
    // SAFETY: this is safe because the pointer is not dropped
    // until the FileProvider is, and we never remove from the
    // list of pointers stored in the vector.
    Ok(unsafe { &*ptr })
  }

  fn resolve(&self, specifier: &str, originating_file: &Path) -> Result<ResolveResult, Self::Error> {
    // Assume the specifier is a relative file path and join it with current path.
    Ok(originating_file.with_file_name(specifier).into())
  }
}

impl Drop for FileProvider {
  fn drop(&mut self) {
    for ptr in self.inputs.lock().unwrap().iter() {
      std::mem::drop(unsafe { Box::from_raw(*ptr) })
    }
  }
}

/// An error that could occur during bundling.
#[derive(Debug)]
#[cfg_attr(any(feature = "serde", feature = "nodejs"), derive(serde::Serialize))]
pub enum BundleErrorKind<'i, T: std::error::Error> {
  /// A parser error occurred.
  ParserError(ParserError<'i>),
  /// An unsupported `@import` condition was encountered.
  UnsupportedImportCondition,
  /// An unsupported cascade layer combination was encountered.
  UnsupportedLayerCombination,
  /// Unsupported media query boolean logic was encountered.
  UnsupportedMediaBooleanLogic,
  /// An external module was referenced with a CSS module "from" clause.
  ReferencedExternalModuleWithCssModuleFrom,
  /// A custom resolver error.
  ResolverError(#[cfg_attr(any(feature = "serde", feature = "nodejs"), serde(skip))] T),
}

impl<'i, T: std::error::Error> From<Error<ParserError<'i>>> for Error<BundleErrorKind<'i, T>> {
  fn from(err: Error<ParserError<'i>>) -> Self {
    Error {
      kind: BundleErrorKind::ParserError(err.kind),
      loc: err.loc,
    }
  }
}

impl<'i, T: std::error::Error> std::fmt::Display for BundleErrorKind<'i, T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    use BundleErrorKind::*;
    match self {
      ParserError(err) => err.fmt(f),
      UnsupportedImportCondition => write!(f, "Unsupported import condition"),
      UnsupportedLayerCombination => write!(f, "Unsupported layer combination in @import"),
      UnsupportedMediaBooleanLogic => write!(f, "Unsupported boolean logic in @import media query"),
      ReferencedExternalModuleWithCssModuleFrom => {
        write!(f, "Referenced external module with CSS module \"from\" clause")
      }
      ResolverError(err) => std::fmt::Display::fmt(&err, f),
    }
  }
}

impl<'i, T: std::error::Error> BundleErrorKind<'i, T> {
  #[deprecated(note = "use `BundleErrorKind::to_string()` or `std::fmt::Display` instead")]
  #[allow(missing_docs)]
  pub fn reason(&self) -> String {
    self.to_string()
  }
}

impl<'a, 'o, 's, P: SourceProvider> Bundler<'a, 'o, 's, P, DefaultAtRuleParser> {
  /// Creates a new Bundler using the given source provider.
  /// If a source map is given, the content of each source file included in the bundle will
  /// be added accordingly.
  pub fn new(
    fs: &'a P,
    source_map: Option<&'s mut SourceMap>,
    options: ParserOptions<'o, 'a>,
  ) -> Bundler<'a, 'o, 's, P, DefaultAtRuleParser> {
    Bundler {
      source_map: source_map.map(Mutex::new),
      fs,
      source_indexes: DashMap::new(),
      stylesheets: Mutex::new(Vec::new()),
      options,
      at_rule_parser: Mutex::new(AtRuleParserValue::Owned(DefaultAtRuleParser)),
    }
  }
}

impl<'a, 'o, 's, P: SourceProvider, T: AtRuleParser<'a> + Clone + Sync + Send> Bundler<'a, 'o, 's, P, T>
where
  T::AtRule: Sync + Send + ToCss + Clone,
{
  /// Creates a new Bundler using the given source provider.
  /// If a source map is given, the content of each source file included in the bundle will
  /// be added accordingly.
  pub fn new_with_at_rule_parser(
    fs: &'a P,
    source_map: Option<&'s mut SourceMap>,
    options: ParserOptions<'o, 'a>,
    at_rule_parser: &'s mut T,
  ) -> Self {
    Bundler {
      source_map: source_map.map(Mutex::new),
      fs,
      source_indexes: DashMap::new(),
      stylesheets: Mutex::new(Vec::new()),
      options,
      at_rule_parser: Mutex::new(AtRuleParserValue::Borrowed(at_rule_parser)),
    }
  }

  /// Bundles the given entry file and all dependencies into a single style sheet.
  pub fn bundle<'e>(
    &mut self,
    entry: &'e Path,
  ) -> Result<StyleSheet<'a, 'o, T::AtRule>, Error<BundleErrorKind<'a, P::Error>>> {
    // Phase 1: load and parse all files. This is done in parallel.
    self.load_file(
      &entry,
      ImportRule {
        url: "".into(),
        layer: None,
        supports: None,
        media: MediaList::new(),
        loc: Location {
          source_index: 0,
          line: 0,
          column: 0,
        },
      },
    )?;

    // Phase 2: determine the order that the files should be concatenated.
    self.order();

    // Phase 3: concatenate.
    let mut rules: Vec<CssRule<'a, T::AtRule>> = Vec::new();
    self.inline(&mut rules);

    let sources = self
      .stylesheets
      .get_mut()
      .unwrap()
      .iter()
      .flat_map(|s| s.stylesheet.as_ref().unwrap().sources.iter().cloned())
      .collect();

    let mut stylesheet = StyleSheet::new(sources, CssRuleList(rules), self.options.clone());

    stylesheet.source_map_urls = self
      .stylesheets
      .get_mut()
      .unwrap()
      .iter()
      .flat_map(|s| s.stylesheet.as_ref().unwrap().source_map_urls.iter().cloned())
      .collect();

    stylesheet.license_comments = self
      .stylesheets
      .get_mut()
      .unwrap()
      .iter()
      .flat_map(|s| s.stylesheet.as_ref().unwrap().license_comments.iter().cloned())
      .collect();

    if let Some(config) = &self.options.css_modules {
      if config.pattern.has_content_hash() {
        stylesheet.content_hashes = Some(
          self
            .stylesheets
            .get_mut()
            .unwrap()
            .iter()
            .flat_map(|s| {
              let s = s.stylesheet.as_ref().unwrap();
              s.content_hashes.as_ref().unwrap().iter().cloned()
            })
            .collect(),
        );
      }
    }

    Ok(stylesheet)
  }

  fn find_filename(&self, source_index: u32) -> String {
    // This function is only used for error handling, so it's ok if this is a bit slow.
    let entry = self.source_indexes.iter().find(|x| *x.value() == source_index).unwrap();
    entry.key().to_str().unwrap().into()
  }

  fn load_file(&self, file: &Path, rule: ImportRule<'a>) -> Result<u32, Error<BundleErrorKind<'a, P::Error>>> {
    // Check if we already loaded this file.
    let mut stylesheets = self.stylesheets.lock().unwrap();
    let source_index = match self.source_indexes.get(file) {
      Some(source_index) => {
        // If we already loaded this file, combine the media queries and supports conditions
        // from this import rule with the existing ones using a logical or operator.
        let entry = &mut stylesheets[*source_index as usize];

        // We cannot combine a media query and a supports query from different @import rules.
        // e.g. @import "a.css" print; @import "a.css" supports(color: red);
        // This would require duplicating the actual rules in the file.
        if (!rule.media.media_queries.is_empty() && !entry.supports.is_none())
          || (!entry.media.media_queries.is_empty() && !rule.supports.is_none())
        {
          return Err(Error {
            kind: BundleErrorKind::UnsupportedImportCondition,
            loc: Some(ErrorLocation::new(rule.loc, self.find_filename(rule.loc.source_index))),
          });
        }

        if rule.media.media_queries.is_empty() {
          entry.media.media_queries.clear();
        } else if !entry.media.media_queries.is_empty() {
          entry.media.or(&rule.media);
        }

        if let Some(supports) = rule.supports {
          if let Some(existing_supports) = &mut entry.supports {
            existing_supports.or(&supports)
          }
        } else {
          entry.supports = None;
        }

        if let Some(layer) = &rule.layer {
          if let Some(existing_layer) = &entry.layer {
            // We can't OR layer names without duplicating all of the nested rules, so error for now.
            if layer != existing_layer || (layer.is_none() && existing_layer.is_none()) {
              return Err(Error {
                kind: BundleErrorKind::UnsupportedLayerCombination,
                loc: Some(ErrorLocation::new(rule.loc, self.find_filename(rule.loc.source_index))),
              });
            }
          } else {
            entry.layer = rule.layer;
          }
        }

        return Ok(*source_index);
      }
      None => {
        let source_index = stylesheets.len() as u32;
        self.source_indexes.insert(file.to_owned(), source_index);

        stylesheets.push(BundleStyleSheet {
          stylesheet: None,
          layer: rule.layer.clone(),
          media: rule.media.clone(),
          supports: rule.supports.clone(),
          loc: rule.loc.clone(),
          dependencies: Vec::new(),
          css_modules_deps: Vec::new(),
          parent_source_index: 0,
          parent_dep_index: 0,
        });

        source_index
      }
    };

    drop(stylesheets); // ensure we aren't holding the lock anymore

    let code = self.fs.read(file).map_err(|e| Error {
      kind: BundleErrorKind::ResolverError(e),
      loc: if rule.loc.column == 0 {
        None
      } else {
        Some(ErrorLocation::new(rule.loc, self.find_filename(rule.loc.source_index)))
      },
    })?;

    let mut opts = self.options.clone();
    let filename = file.to_str().unwrap();
    opts.filename = filename.to_owned();
    opts.source_index = source_index;

    let mut stylesheet = {
      let mut at_rule_parser = self.at_rule_parser.lock().unwrap();
      let at_rule_parser = match &mut *at_rule_parser {
        AtRuleParserValue::Owned(owned) => owned,
        AtRuleParserValue::Borrowed(borrowed) => *borrowed,
      };

      StyleSheet::<T::AtRule>::parse_with(code, opts, at_rule_parser)?
    };

    if let Some(source_map) = &self.source_map {
      // Only add source if we don't have an input source map.
      // If we do, this will be handled by the printer when remapping locations.
      let sm = stylesheet.source_map_url(0);
      if sm.is_none() || !sm.unwrap().starts_with("data") {
        let mut source_map = source_map.lock().unwrap();
        let source_index = source_map.add_source(filename);
        let _ = source_map.set_source_content(source_index as usize, code);
      }
    }

    // Collect and load dependencies for this stylesheet in parallel.
    let dependencies: Result<Vec<Dependency>, _> = stylesheet
      .rules
      .0
      .par_iter_mut()
      .filter_map(|r| {
        // Prepend parent layer name to @layer statements.
        if let CssRule::LayerStatement(layer) = r {
          if let Some(Some(parent_layer)) = &rule.layer {
            for name in &mut layer.names {
              name.0.insert_many(0, parent_layer.0.iter().cloned())
            }
          }
        }

        if let CssRule::Import(import) = r {
          let specifier = &import.url;

          // Combine media queries and supports conditions from parent
          // stylesheet with @import rule using a logical and operator.
          let mut media = rule.media.clone();
          let result = media.and(&import.media).map_err(|_| Error {
            kind: BundleErrorKind::UnsupportedMediaBooleanLogic,
            loc: Some(ErrorLocation::new(
              import.loc,
              self.find_filename(import.loc.source_index),
            )),
          });

          if let Err(e) = result {
            return Some(Err(e));
          }

          let layer = if (rule.layer == Some(None) && import.layer.is_some())
            || (import.layer == Some(None) && rule.layer.is_some())
          {
            // Cannot combine anonymous layers
            return Some(Err(Error {
              kind: BundleErrorKind::UnsupportedLayerCombination,
              loc: Some(ErrorLocation::new(
                import.loc,
                self.find_filename(import.loc.source_index),
              )),
            }));
          } else if let Some(Some(a)) = &rule.layer {
            if let Some(Some(b)) = &import.layer {
              let mut name = a.clone();
              name.0.extend(b.0.iter().cloned());
              Some(Some(name))
            } else {
              Some(Some(a.clone()))
            }
          } else {
            import.layer.clone()
          };

          let result = match self.fs.resolve(&specifier, file) {
            Ok(ResolveResult::File(path)) => self
              .load_file(
                &path,
                ImportRule {
                  layer,
                  media,
                  supports: combine_supports(rule.supports.clone(), &import.supports),
                  url: "".into(),
                  loc: import.loc,
                },
              )
              .map(Dependency::File),
            Ok(ResolveResult::External(url)) => Ok(Dependency::External(url)),
            Err(err) => Err(Error {
              kind: BundleErrorKind::ResolverError(err),
              loc: Some(ErrorLocation::new(
                import.loc,
                self.find_filename(import.loc.source_index),
              )),
            }),
          };

          Some(result)
        } else {
          None
        }
      })
      .collect();

    // Collect CSS modules dependencies from the `composes` property.
    let css_modules_deps: Result<Vec<u32>, _> = if self.options.css_modules.is_some() {
      stylesheet
        .rules
        .0
        .par_iter_mut()
        .filter_map(|r| {
          if let CssRule::Style(style) = r {
            Some(
              style
                .declarations
                .declarations
                .par_iter_mut()
                .chain(style.declarations.important_declarations.par_iter_mut())
                .filter_map(|d| match d {
                  Property::Composes(composes) => self
                    .add_css_module_dep(file, &rule, style.loc, composes.loc, &mut composes.from)
                    .map(|result| rayon::iter::Either::Left(rayon::iter::once(result))),

                  // Handle variable references if the dashed_idents option is present.
                  Property::Custom(CustomProperty { value, .. })
                  | Property::Unparsed(UnparsedProperty { value, .. })
                    if matches!(&self.options.css_modules, Some(css_modules) if css_modules.dashed_idents) =>
                  {
                    Some(rayon::iter::Either::Right(visit_vars(value).filter_map(|name| {
                      self.add_css_module_dep(
                        file,
                        &rule,
                        style.loc,
                        // TODO: store loc in variable reference?
                        crate::dependencies::Location {
                          line: style.loc.line,
                          column: style.loc.column,
                        },
                        &mut name.from,
                      )
                    })))
                  }
                  _ => None,
                })
                .flatten(),
            )
          } else {
            None
          }
        })
        .flatten()
        .collect()
    } else {
      Ok(vec![])
    };

    let entry = &mut self.stylesheets.lock().unwrap()[source_index as usize];
    entry.stylesheet = Some(stylesheet);
    entry.dependencies = dependencies?;
    entry.css_modules_deps = css_modules_deps?;

    Ok(source_index)
  }

  fn add_css_module_dep(
    &self,
    file: &Path,
    rule: &ImportRule<'a>,
    style_loc: Location,
    loc: crate::dependencies::Location,
    specifier: &mut Option<Specifier>,
  ) -> Option<Result<u32, Error<BundleErrorKind<'a, P::Error>>>> {
    if let Some(Specifier::File(f)) = specifier {
      let result = match self.fs.resolve(&f, file) {
        Ok(ResolveResult::File(path)) => {
          let res = self.load_file(
            &path,
            ImportRule {
              layer: rule.layer.clone(),
              media: rule.media.clone(),
              supports: rule.supports.clone(),
              url: "".into(),
              loc: Location {
                source_index: style_loc.source_index,
                line: loc.line,
                column: loc.column,
              },
            },
          );

          if let Ok(source_index) = res {
            *specifier = Some(Specifier::SourceIndex(source_index));
          }

          res
        }
        Ok(ResolveResult::External(_)) => Err(Error {
          kind: BundleErrorKind::ReferencedExternalModuleWithCssModuleFrom,
          loc: Some(ErrorLocation::new(
            style_loc,
            self.find_filename(style_loc.source_index),
          )),
        }),
        Err(err) => Err(Error {
          kind: BundleErrorKind::ResolverError(err),
          loc: Some(ErrorLocation::new(
            style_loc,
            self.find_filename(style_loc.source_index),
          )),
        }),
      };
      Some(result)
    } else {
      None
    }
  }

  fn order(&mut self) {
    process(self.stylesheets.get_mut().unwrap(), 0, &mut HashSet::new());

    fn process<'i, T>(
      stylesheets: &mut Vec<BundleStyleSheet<'i, '_, T>>,
      source_index: u32,
      visited: &mut HashSet<u32>,
    ) {
      if visited.contains(&source_index) {
        return;
      }

      visited.insert(source_index);

      let mut dep_index = 0;
      for i in 0..stylesheets[source_index as usize].css_modules_deps.len() {
        let dep_source_index = stylesheets[source_index as usize].css_modules_deps[i];
        let resolved = &mut stylesheets[dep_source_index as usize];

        // CSS modules preserve the first instance of composed stylesheets.
        if !visited.contains(&dep_source_index) {
          resolved.parent_dep_index = dep_index;
          resolved.parent_source_index = source_index;
          process(stylesheets, dep_source_index, visited);
        }

        dep_index += 1;
      }

      for i in 0..stylesheets[source_index as usize].dependencies.len() {
        let Dependency::File(dep_source_index) = stylesheets[source_index as usize].dependencies[i] else {
          continue;
        };
        let resolved = &mut stylesheets[dep_source_index as usize];

        // In browsers, every instance of an @import is evaluated, so we preserve the last.
        resolved.parent_dep_index = dep_index;
        resolved.parent_source_index = source_index;

        process(stylesheets, dep_source_index, visited);
        dep_index += 1;
      }
    }
  }

  fn inline(&mut self, dest: &mut Vec<CssRule<'a, T::AtRule>>) {
    process(self.stylesheets.get_mut().unwrap(), 0, dest);

    fn process<'a, T>(
      stylesheets: &mut Vec<BundleStyleSheet<'a, '_, T>>,
      source_index: u32,
      dest: &mut Vec<CssRule<'a, T>>,
    ) {
      let stylesheet = &mut stylesheets[source_index as usize];
      let mut rules = std::mem::take(&mut stylesheet.stylesheet.as_mut().unwrap().rules.0);

      // Hoist css modules deps
      let mut dep_index = 0;
      for i in 0..stylesheet.css_modules_deps.len() {
        let dep_source_index = stylesheets[source_index as usize].css_modules_deps[i];
        let resolved = &stylesheets[dep_source_index as usize];

        // Include the dependency if this is the first instance as computed earlier.
        if resolved.parent_source_index == source_index && resolved.parent_dep_index == dep_index as u32 {
          process(stylesheets, dep_source_index, dest);
        }

        dep_index += 1;
      }

      let mut import_index = 0;
      for rule in &mut rules {
        match rule {
          CssRule::Import(import_rule) => {
            let dep_source = &stylesheets[source_index as usize].dependencies[import_index];
            match dep_source {
              Dependency::File(dep_source_index) => {
                let resolved = &stylesheets[*dep_source_index as usize];

                // Include the dependency if this is the last instance as computed earlier.
                if resolved.parent_source_index == source_index && resolved.parent_dep_index == dep_index {
                  process(stylesheets, *dep_source_index, dest);
                }

                *rule = CssRule::Ignored;
                dep_index += 1;
              }
              Dependency::External(url) => {
                import_rule.url = url.to_owned().into();
                let imp = std::mem::replace(rule, CssRule::Ignored);
                dest.push(imp);
              }
            }
            import_index += 1;
          }
          CssRule::LayerStatement(_) => {
            // @layer rules are the only rules that may appear before an @import.
            // We must preserve this order to ensure correctness.
            let layer = std::mem::replace(rule, CssRule::Ignored);
            dest.push(layer);
          }
          CssRule::Ignored => {}
          _ => break,
        }
      }

      // Wrap rules in the appropriate @layer, @media, and @supports rules.
      let stylesheet = &mut stylesheets[source_index as usize];

      if stylesheet.layer.is_some() {
        rules = vec![CssRule::LayerBlock(LayerBlockRule {
          name: stylesheet.layer.take().unwrap(),
          rules: CssRuleList(rules),
          loc: stylesheet.loc,
        })]
      }

      if !stylesheet.media.media_queries.is_empty() {
        rules = vec![CssRule::Media(MediaRule {
          query: std::mem::replace(&mut stylesheet.media, MediaList::new()),
          rules: CssRuleList(rules),
          loc: stylesheet.loc,
        })]
      }

      if stylesheet.supports.is_some() {
        rules = vec![CssRule::Supports(SupportsRule {
          condition: stylesheet.supports.take().unwrap(),
          rules: CssRuleList(rules),
          loc: stylesheet.loc,
        })]
      }

      dest.extend(rules);
    }
  }
}

fn combine_supports<'a>(
  a: Option<SupportsCondition<'a>>,
  b: &Option<SupportsCondition<'a>>,
) -> Option<SupportsCondition<'a>> {
  if let Some(mut a) = a {
    if let Some(b) = b {
      a.and(b)
    }
    Some(a)
  } else {
    b.clone()
  }
}

fn visit_vars<'a, 'b>(
  token_list: &'b mut TokenList<'a>,
) -> impl ParallelIterator<Item = &'b mut DashedIdentReference<'a>> {
  let mut stack = vec![token_list.0.iter_mut()];
  std::iter::from_fn(move || {
    while !stack.is_empty() {
      let iter = stack.last_mut().unwrap();
      match iter.next() {
        Some(TokenOrValue::Var(var)) => {
          if let Some(fallback) = &mut var.fallback {
            stack.push(fallback.0.iter_mut());
          }
          return Some(&mut var.name);
        }
        Some(TokenOrValue::Env(env)) => {
          if let Some(fallback) = &mut env.fallback {
            stack.push(fallback.0.iter_mut());
          }
          if let EnvironmentVariableName::Custom(name) = &mut env.name {
            return Some(name);
          }
        }
        Some(TokenOrValue::UnresolvedColor(color)) => match color {
          UnresolvedColor::RGB { alpha, .. } | UnresolvedColor::HSL { alpha, .. } => {
            stack.push(alpha.0.iter_mut());
          }
          UnresolvedColor::LightDark { light, dark } => {
            stack.push(light.0.iter_mut());
            stack.push(dark.0.iter_mut());
          }
        },
        None => {
          stack.pop();
        }
        _ => {}
      }
    }
    None
  })
  .par_bridge()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    css_modules::{self, CssModuleExports, CssModuleReference},
    parser::ParserFlags,
    stylesheet::{MinifyOptions, PrinterOptions},
    targets::{Browsers, Targets},
  };
  use indoc::indoc;
  use std::collections::HashMap;

  #[derive(Clone)]
  struct TestProvider {
    map: HashMap<PathBuf, String>,
  }

  impl SourceProvider for TestProvider {
    type Error = std::io::Error;

    fn read<'a>(&'a self, file: &Path) -> Result<&'a str, Self::Error> {
      Ok(self.map.get(file).unwrap())
    }

    fn resolve(&self, specifier: &str, originating_file: &Path) -> Result<ResolveResult, Self::Error> {
      if specifier.starts_with("https:") {
        Ok(ResolveResult::External(specifier.to_owned()))
      } else {
        Ok(originating_file.with_file_name(specifier).into())
      }
    }
  }

  /// Stand-in for a user-authored `SourceProvider` with application-specific logic.
  struct CustomProvider {
    map: HashMap<PathBuf, String>,
  }

  impl SourceProvider for CustomProvider {
    type Error = std::io::Error;

    /// Read files from in-memory map.
    fn read<'a>(&'a self, file: &Path) -> Result<&'a str, Self::Error> {
      Ok(self.map.get(file).unwrap())
    }

    /// Resolve by stripping a `foo:` prefix off any import. Specifiers without
    /// this prefix fail with an error.
    fn resolve(&self, specifier: &str, _originating_file: &Path) -> Result<ResolveResult, Self::Error> {
      if specifier.starts_with("foo:") {
        Ok(Path::new(&specifier["foo:".len()..]).to_path_buf().into())
      } else {
        let err = std::io::Error::new(
          std::io::ErrorKind::NotFound,
          format!(
            "Failed to resolve `{}`, specifier does not start with `foo:`.",
            &specifier
          ),
        );

        Err(err)
      }
    }
  }

  macro_rules! fs(
    { $($key:literal: $value:expr),* } => {
      {
        #[allow(unused_mut)]
        let mut m = HashMap::new();
        $(
          m.insert(PathBuf::from($key), $value.to_owned());
        )*
        m
      }
    };
  );

  fn bundle<P: SourceProvider>(fs: P, entry: &str) -> String {
    let mut bundler = Bundler::new(&fs, None, ParserOptions::default());
    let stylesheet = bundler.bundle(Path::new(entry)).unwrap();
    stylesheet.to_css(PrinterOptions::default()).unwrap().code
  }

  fn bundle_css_module<P: SourceProvider>(
    fs: P,
    entry: &str,
    project_root: Option<&str>,
  ) -> (String, CssModuleExports) {
    bundle_css_module_with_pattern(fs, entry, project_root, "[hash]_[local]")
  }

  fn bundle_css_module_with_pattern<P: SourceProvider>(
    fs: P,
    entry: &str,
    project_root: Option<&str>,
    pattern: &'static str,
  ) -> (String, CssModuleExports) {
    let mut bundler = Bundler::new(
      &fs,
      None,
      ParserOptions {
        css_modules: Some(css_modules::Config {
          dashed_idents: true,
          pattern: css_modules::Pattern::parse(pattern).unwrap(),
          ..Default::default()
        }),
        ..ParserOptions::default()
      },
    );
    let mut stylesheet = bundler.bundle(Path::new(entry)).unwrap();
    stylesheet.minify(MinifyOptions::default()).unwrap();
    let res = stylesheet
      .to_css(PrinterOptions {
        project_root,
        ..PrinterOptions::default()
      })
      .unwrap();
    (res.code, res.exports.unwrap())
  }

  fn bundle_custom_media<P: SourceProvider>(fs: P, entry: &str) -> String {
    let mut bundler = Bundler::new(
      &fs,
      None,
      ParserOptions {
        flags: ParserFlags::CUSTOM_MEDIA,
        ..ParserOptions::default()
      },
    );
    let mut stylesheet = bundler.bundle(Path::new(entry)).unwrap();
    let targets = Targets {
      browsers: Some(Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      }),
      ..Default::default()
    };
    stylesheet
      .minify(MinifyOptions {
        targets,
        ..MinifyOptions::default()
      })
      .unwrap();
    stylesheet
      .to_css(PrinterOptions {
        targets,
        ..PrinterOptions::default()
      })
      .unwrap()
      .code
  }

  fn error_test<P: SourceProvider>(
    fs: P,
    entry: &str,
    maybe_cb: Option<Box<dyn FnOnce(BundleErrorKind<P::Error>) -> ()>>,
  ) {
    let mut bundler = Bundler::new(&fs, None, ParserOptions::default());
    let res = bundler.bundle(Path::new(entry));
    match res {
      Ok(_) => unreachable!(),
      Err(e) => {
        if let Some(cb) = maybe_cb {
          cb(e.kind);
        }
      }
    }
  }

  fn flatten_exports(exports: CssModuleExports) -> HashMap<String, String> {
    let mut res = HashMap::new();
    for (name, export) in &exports {
      let mut classes = export.name.clone();
      for composes in &export.composes {
        classes.push(' ');
        classes.push_str(match composes {
          CssModuleReference::Local { name } => name,
          CssModuleReference::Global { name } => name,
          _ => unreachable!(),
        })
      }
      res.insert(name.clone(), classes);
    }
    res
  }

  #[test]
  fn test_bundle() {
    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css";
          .a { color: red }
        "#,
          "/b.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      .b {
        color: green;
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" print;
          .a { color: red }
        "#,
          "/b.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @media print {
        .b {
          color: green;
        }
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" supports(color: green);
          .a { color: red }
        "#,
          "/b.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @supports (color: green) {
        .b {
          color: green;
        }
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" supports(color: green) print;
          .a { color: red }
        "#,
          "/b.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @supports (color: green) {
        @media print {
          .b {
            color: green;
          }
        }
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" print;
          @import "b.css" screen;
          .a { color: red }
        "#,
          "/b.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @media print, screen {
        .b {
          color: green;
        }
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" supports(color: red);
          @import "b.css" supports(foo: bar);
          .a { color: red }
        "#,
          "/b.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @supports (color: red) or (foo: bar) {
        .b {
          color: green;
        }
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" print;
          .a { color: red }
        "#,
          "/b.css": r#"
          @import "c.css" (color);
          .b { color: yellow }
        "#,
          "/c.css": r#"
          .c { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @media print and (color) {
        .c {
          color: green;
        }
      }

      @media print {
        .b {
          color: #ff0;
        }
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css";
          .a { color: red }
        "#,
          "/b.css": r#"
          @import "c.css";
        "#,
          "/c.css": r#"
          @import "a.css";
          .c { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      .c {
        color: green;
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b/c.css";
          .a { color: red }
        "#,
          "/b/c.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      .b {
        color: green;
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "./b/c.css";
          .a { color: red }
        "#,
          "/b/c.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      .b {
        color: green;
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle_custom_media(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "media.css";
          @import "b.css";
          .a { color: red }
        "#,
          "/media.css": r#"
          @custom-media --foo print;
        "#,
          "/b.css": r#"
          @media (--foo) {
            .a { color: green }
          }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @media print {
        .a {
          color: green;
        }
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" layer(foo);
          .a { color: red }
        "#,
          "/b.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @layer foo {
        .b {
          color: green;
        }
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" layer;
          .a { color: red }
        "#,
          "/b.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @layer {
        .b {
          color: green;
        }
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" layer(foo);
          .a { color: red }
        "#,
          "/b.css": r#"
          @import "c.css" layer(bar);
          .b { color: green }
        "#,
          "/c.css": r#"
          .c { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @layer foo.bar {
        .c {
          color: green;
        }
      }

      @layer foo {
        .b {
          color: green;
        }
      }

      .a {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" layer(foo);
          @import "b.css" layer(foo);
        "#,
          "/b.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @layer foo {
        .b {
          color: green;
        }
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @layer bar, foo;
          @import "b.css" layer(foo);

          @layer bar {
            div {
              background: red;
            }
          }
        "#,
          "/b.css": r#"
          @layer qux, baz;
          @import "c.css" layer(baz);

          @layer qux {
            div {
              background: green;
            }
          }
        "#,
          "/c.css": r#"
          div {
            background: yellow;
          }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @layer bar, foo;
      @layer foo.qux, foo.baz;

      @layer foo.baz {
        div {
          background: #ff0;
        }
      }

      @layer foo {
        @layer qux {
          div {
            background: green;
          }
        }
      }

      @layer bar {
        div {
          background: red;
        }
      }
    "#}
    );

    // Layer order depends on @import conditions.
    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" layer(bar) (min-width: 1000px);

          @layer baz {
            #box { background: purple }
          }

          @layer bar {
            #box { background: yellow }
          }
        "#,
          "/b.css": r#"
          #box { background: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      @media (width >= 1000px) {
        @layer bar {
          #box {
            background: green;
          }
        }
      }

      @layer baz {
        #box {
          background: purple;
        }
      }

      @layer bar {
        #box {
          background: #ff0;
        }
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import url('https://fonts.googleapis.com/css2?family=Roboto&display=swap');
          @import './b.css';
        "#,
          "/b.css": r#"
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
        @import "https://fonts.googleapis.com/css2?family=Roboto&display=swap";

        .b {
          color: green;
        }
    "#}
    );

    error_test(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" layer(foo);
          @import "b.css" layer(bar);
        "#,
          "/b.css": r#"
          .b { color: red }
        "#
        },
      },
      "/a.css",
      Some(Box::new(|err| {
        assert!(matches!(err, BundleErrorKind::UnsupportedLayerCombination));
      })),
    );

    error_test(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" layer;
          @import "b.css" layer;
        "#,
          "/b.css": r#"
          .b { color: red }
        "#
        },
      },
      "/a.css",
      Some(Box::new(|err| {
        assert!(matches!(err, BundleErrorKind::UnsupportedLayerCombination));
      })),
    );

    error_test(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" layer;
          .a { color: red }
        "#,
          "/b.css": r#"
          @import "c.css" layer;
          .b { color: green }
        "#,
          "/c.css": r#"
          .c { color: green }
        "#
        },
      },
      "/a.css",
      Some(Box::new(|err| {
        assert!(matches!(err, BundleErrorKind::UnsupportedLayerCombination));
      })),
    );

    error_test(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css" layer;
          .a { color: red }
        "#,
          "/b.css": r#"
          @import "c.css" layer(foo);
          .b { color: green }
        "#,
          "/c.css": r#"
          .c { color: green }
        "#
        },
      },
      "/a.css",
      Some(Box::new(|err| {
        assert!(matches!(err, BundleErrorKind::UnsupportedLayerCombination));
      })),
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/index.css": r#"
          @import "a.css";
          @import "b.css";
        "#,
          "/a.css": r#"
          @import "./c.css";
          body { background: red; }
        "#,
          "/b.css": r#"
          @import "./c.css";
          body { color: red; }
        "#,
          "/c.css": r#"
          body {
            background: white;
            color: black;
          }
        "#
        },
      },
      "/index.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      body {
        background: red;
      }

      body {
        background: #fff;
        color: #000;
      }

      body {
        color: red;
      }
    "#}
    );

    let res = bundle(
      TestProvider {
        map: fs! {
          "/index.css": r#"
          @import "a.css";
          @import "b.css";
          @import "a.css";
        "#,
          "/a.css": r#"
          body { background: green; }
        "#,
          "/b.css": r#"
          body { background: red; }
        "#
        },
      },
      "/index.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      body {
        background: red;
      }

      body {
        background: green;
      }
    "#}
    );

    let res = bundle(
      CustomProvider {
        map: fs! {
          "/a.css": r#"
            @import "foo:/b.css";
            .a { color: red; }
          "#,
          "/b.css": ".b { color: green; }"
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
        .b {
          color: green;
        }

        .a {
          color: red;
        }
      "# }
    );

    error_test(
      CustomProvider {
        map: fs! {
          "/a.css": r#"
            /* Forgot to prefix with `foo:`. */
            @import "/b.css";
            .a { color: red; }
          "#,
          "/b.css": ".b { color: green; }"
        },
      },
      "/a.css",
      Some(Box::new(|err| {
        let kind = match err {
          BundleErrorKind::ResolverError(ref error) => error.kind(),
          _ => unreachable!(),
        };
        assert!(matches!(kind, std::io::ErrorKind::NotFound));
        assert!(err
          .to_string()
          .contains("Failed to resolve `/b.css`, specifier does not start with `foo:`."));
      })),
    );

    // let res = bundle(fs! {
    //   "/a.css": r#"
    //     @import "b.css" supports(color: red) (color);
    //     @import "b.css" supports(foo: bar) (orientation: horizontal);
    //     .a { color: red }
    //   "#,
    //   "/b.css": r#"
    //     .b { color: green }
    //   "#
    // }, "/a.css");

    // let res = bundle(fs! {
    //   "/a.css": r#"
    //     @import "b.css" not print;
    //     .a { color: red }
    //   "#,
    //   "/b.css": r#"
    //     @import "c.css" not screen;
    //     .b { color: green }
    //   "#,
    //   "/c.css": r#"
    //     .c { color: yellow }
    //   "#
    // }, "/a.css");
  }

  #[test]
  fn test_css_module() {
    macro_rules! map {
      { $($key:expr => $val:expr),* } => {
        HashMap::from([
          $(($key.to_owned(), $val.to_owned()),)*
        ])
      };
    }

    let (code, exports) = bundle_css_module(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css";
          .a { color: red }
        "#,
          "/b.css": r#"
          .a { color: green }
        "#
        },
      },
      "/a.css",
      None,
    );
    assert_eq!(
      code,
      indoc! { r#"
      ._9z6RGq_a {
        color: green;
      }

      ._6lixEq_a {
        color: red;
      }
    "#}
    );
    assert_eq!(
      flatten_exports(exports),
      map! {
        "a" => "_6lixEq_a"
      }
    );

    let (code, exports) = bundle_css_module(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          .a { composes: x from './b.css'; color: red; }
          .b { color: yellow }
        "#,
          "/b.css": r#"
          .x { composes: y; background: green }
          .y { font: Helvetica }
        "#
        },
      },
      "/a.css",
      None,
    );
    assert_eq!(
      code,
      indoc! { r#"
      ._8Cs9ZG_x {
        background: green;
      }

      ._8Cs9ZG_y {
        font: Helvetica;
      }

      ._6lixEq_a {
        color: red;
      }

      ._6lixEq_b {
        color: #ff0;
      }
    "#}
    );
    assert_eq!(
      flatten_exports(exports),
      map! {
        "a" => "_6lixEq_a _8Cs9ZG_x _8Cs9ZG_y",
        "b" => "_6lixEq_b"
      }
    );

    let (code, exports) = bundle_css_module(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          .a { composes: x from './b.css'; background: red; }
        "#,
          "/b.css": r#"
          .a { background: red }
        "#
        },
      },
      "/a.css",
      None,
    );
    assert_eq!(
      code,
      indoc! { r#"
      ._8Cs9ZG_a {
        background: red;
      }

      ._6lixEq_a {
        background: red;
      }
    "#}
    );
    assert_eq!(
      flatten_exports(exports),
      map! {
        "a" => "_6lixEq_a"
      }
    );

    let (code, exports) = bundle_css_module(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          .a {
            background: var(--bg from "./b.css", var(--fallback from "./b.css"));
            color: rgb(255 255 255 / var(--opacity from "./b.css"));
            width: env(--env, var(--env-fallback from "./env.css"));
          }
        "#,
          "/b.css": r#"
          .b {
            --bg: red;
            --fallback: yellow;
            --opacity: 0.5;
          }
        "#,
          "/env.css": r#"
          .env {
            --env-fallback: 20px;
          }
        "#
        },
      },
      "/a.css",
      None,
    );
    assert_eq!(
      code,
      indoc! { r#"
      ._8Cs9ZG_b {
        --_8Cs9ZG_bg: red;
        --_8Cs9ZG_fallback: yellow;
        --_8Cs9ZG_opacity: .5;
      }

      .GbJUva_env {
        --GbJUva_env-fallback: 20px;
      }

      ._6lixEq_a {
        background: var(--_8Cs9ZG_bg, var(--_8Cs9ZG_fallback));
        color: rgb(255 255 255 / var(--_8Cs9ZG_opacity));
        width: env(--_6lixEq_env, var(--GbJUva_env-fallback));
      }
    "#}
    );
    assert_eq!(
      flatten_exports(exports),
      map! {
        "a" => "_6lixEq_a",
        "--env" => "--_6lixEq_env"
      }
    );

    // Hashes are stable between project roots.
    let expected = indoc! { r#"
    .dyGcAa_b {
      background: #ff0;
    }

    .CK9avG_a {
      background: #fff;
    }
  "#};

    let (code, _) = bundle_css_module(
      TestProvider {
        map: fs! {
          "/foo/bar/a.css": r#"
        @import "b.css";
        .a {
          background: white;
        }
      "#,
          "/foo/bar/b.css": r#"
        .b {
          background: yellow;
        }
      "#
        },
      },
      "/foo/bar/a.css",
      Some("/foo/bar"),
    );
    assert_eq!(code, expected);

    let (code, _) = bundle_css_module(
      TestProvider {
        map: fs! {
          "/x/y/z/a.css": r#"
      @import "b.css";
      .a {
        background: white;
      }
    "#,
          "/x/y/z/b.css": r#"
      .b {
        background: yellow;
      }
    "#
        },
      },
      "/x/y/z/a.css",
      Some("/x/y/z"),
    );
    assert_eq!(code, expected);

    let (code, _) = bundle_css_module_with_pattern(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          @import "b.css";
          .a { color: red }
        "#,
          "/b.css": r#"
          .a { color: green }
        "#
        },
      },
      "/a.css",
      None,
      "[content-hash]-[local]",
    );
    assert_eq!(
      code,
      indoc! { r#"
      .do5n2W-a {
        color: green;
      }

      .pP97eq-a {
        color: red;
      }
    "#}
    );
  }

  #[test]
  fn test_source_map() {
    let source = r#".imported {
      content: "yay, file support!";
    }

    .selector {
      margin: 1em;
      background-color: #f60;
    }

    .selector .nested {
      margin: 0.5em;
    }

    /*# sourceMappingURL=data:application/json;base64,ewoJInZlcnNpb24iOiAzLAoJInNvdXJjZVJvb3QiOiAicm9vdCIsCgkiZmlsZSI6ICJzdGRvdXQiLAoJInNvdXJjZXMiOiBbCgkJInN0ZGluIiwKCQkic2Fzcy9fdmFyaWFibGVzLnNjc3MiLAoJCSJzYXNzL19kZW1vLnNjc3MiCgldLAoJInNvdXJjZXNDb250ZW50IjogWwoJCSJAaW1wb3J0IFwiX3ZhcmlhYmxlc1wiO1xuQGltcG9ydCBcIl9kZW1vXCI7XG5cbi5zZWxlY3RvciB7XG4gIG1hcmdpbjogJHNpemU7XG4gIGJhY2tncm91bmQtY29sb3I6ICRicmFuZENvbG9yO1xuXG4gIC5uZXN0ZWQge1xuICAgIG1hcmdpbjogJHNpemUgLyAyO1xuICB9XG59IiwKCQkiJGJyYW5kQ29sb3I6ICNmNjA7XG4kc2l6ZTogMWVtOyIsCgkJIi5pbXBvcnRlZCB7XG4gIGNvbnRlbnQ6IFwieWF5LCBmaWxlIHN1cHBvcnQhXCI7XG59IgoJXSwKCSJtYXBwaW5ncyI6ICJBRUFBLFNBQVMsQ0FBQztFQUNSLE9BQU8sRUFBRSxvQkFBcUI7Q0FDL0I7O0FGQ0QsU0FBUyxDQUFDO0VBQ1IsTUFBTSxFQ0hELEdBQUc7RURJUixnQkFBZ0IsRUNMTCxJQUFJO0NEVWhCOztBQVBELFNBQVMsQ0FJUCxPQUFPLENBQUM7RUFDTixNQUFNLEVDUEgsS0FBRztDRFFQIiwKCSJuYW1lcyI6IFtdCn0= */"#;

    let fs = TestProvider {
      map: fs! {
        "/a.css": r#"
        @import "/b.css";
        .a { color: red; }
      "#,
        "/b.css": source
      },
    };

    let mut sm = parcel_sourcemap::SourceMap::new("/");
    let mut bundler = Bundler::new(&fs, Some(&mut sm), ParserOptions::default());
    let mut stylesheet = bundler.bundle(Path::new("/a.css")).unwrap();
    stylesheet.minify(MinifyOptions::default()).unwrap();
    stylesheet
      .to_css(PrinterOptions {
        source_map: Some(&mut sm),
        minify: true,
        ..PrinterOptions::default()
      })
      .unwrap();
    let map = sm.to_json(None).unwrap();
    assert_eq!(
      map,
      r#"{"version":3,"sourceRoot":null,"mappings":"ACAA,uCCGA,2CAAA,8BFDQ","sources":["a.css","sass/_demo.scss","stdin"],"sourcesContent":["\n        @import \"/b.css\";\n        .a { color: red; }\n      ",".imported {\n  content: \"yay, file support!\";\n}","@import \"_variables\";\n@import \"_demo\";\n\n.selector {\n  margin: $size;\n  background-color: $brandColor;\n\n  .nested {\n    margin: $size / 2;\n  }\n}"],"names":[]}"#
    );
  }

  #[test]
  fn test_license_comments() {
    let res = bundle(
      TestProvider {
        map: fs! {
          "/a.css": r#"
          /*! Copyright 2023 Someone awesome */
          @import "b.css";
          .a { color: red }
        "#,
          "/b.css": r#"
          /*! Copyright 2023 Someone else */
          .b { color: green }
        "#
        },
      },
      "/a.css",
    );
    assert_eq!(
      res,
      indoc! { r#"
      /*! Copyright 2023 Someone awesome */
      /*! Copyright 2023 Someone else */
      .b {
        color: green;
      }

      .a {
        color: red;
      }
    "#}
    );
  }
}
