//! CSS style sheets and style attributes.
//!
//! A [StyleSheet](StyleSheet) represents a `.css` file or `<style>` element in HTML.
//! A [StyleAttribute](StyleAttribute) represents an inline `style` attribute in HTML.

use crate::context::{DeclarationContext, PropertyHandlerContext};
use crate::css_modules::{hash, CssModule, CssModuleExports, CssModuleReferences};
use crate::declaration::{DeclarationBlock, DeclarationHandler};
use crate::dependencies::Dependency;
use crate::error::{Error, ErrorLocation, MinifyErrorKind, ParserError, PrinterError, PrinterErrorKind};
use crate::parser::{DefaultAtRule, DefaultAtRuleParser, TopLevelRuleParser};
use crate::printer::Printer;
use crate::rules::{CssRule, CssRuleList, MinifyContext};
use crate::targets::{should_compile, Targets};
use crate::traits::{AtRuleParser, ToCss};
use crate::values::string::CowArcStr;
#[cfg(feature = "visitor")]
use crate::visitor::{Visit, VisitTypes, Visitor};
use cssparser::{Parser, ParserInput, StyleSheetParser};
#[cfg(feature = "sourcemap")]
use parcel_sourcemap::SourceMap;
use std::collections::{HashMap, HashSet};

pub use crate::parser::{ParserFlags, ParserOptions};
pub use crate::printer::PrinterOptions;
pub use crate::printer::PseudoClasses;

/// A CSS style sheet, representing a `.css` file or inline `<style>` element.
///
/// Style sheets can be parsed from a string, constructed from scratch,
/// or created using a [Bundler](super::bundler::Bundler). Then, they can be
/// minified and transformed for a set of target browsers, and serialied to a string.
///
/// # Example
///
/// ```
/// use lightningcss::stylesheet::{
///   StyleSheet, ParserOptions, MinifyOptions, PrinterOptions
/// };
///
/// // Parse a style sheet from a string.
/// let mut stylesheet = StyleSheet::parse(
///   r#"
///   .foo {
///     color: red;
///   }
///
///   .bar {
///     color: red;
///   }
///   "#,
///   ParserOptions::default()
/// ).unwrap();
///
/// // Minify the stylesheet.
/// stylesheet.minify(MinifyOptions::default()).unwrap();
///
/// // Serialize it to a string.
/// let res = stylesheet.to_css(PrinterOptions::default()).unwrap();
/// assert_eq!(res.code, ".foo, .bar {\n  color: red;\n}\n");
/// ```
#[derive(Debug)]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(rename_all = "camelCase")
)]
#[cfg_attr(
  feature = "jsonschema",
  derive(schemars::JsonSchema),
  schemars(rename = "StyleSheet", bound = "T: schemars::JsonSchema")
)]
pub struct StyleSheet<'i, 'o, T = DefaultAtRule> {
  /// A list of top-level rules within the style sheet.
  #[cfg_attr(feature = "serde", serde(borrow))]
  pub rules: CssRuleList<'i, T>,
  /// A list of file names for all source files included within the style sheet.
  /// Sources are referenced by index in the `loc` property of each rule.
  pub sources: Vec<String>,
  /// The source map URL extracted from the original style sheet.
  pub(crate) source_map_urls: Vec<Option<String>>,
  /// The license comments that appeared at the start of the file.
  pub license_comments: Vec<CowArcStr<'i>>,
  /// A list of content hashes for all source files included within the style sheet.
  /// This is only set if CSS modules are enabled and the pattern includes [content-hash].
  #[cfg_attr(feature = "serde", serde(skip))]
  pub(crate) content_hashes: Option<Vec<String>>,
  #[cfg_attr(feature = "serde", serde(skip))]
  /// The options the style sheet was originally parsed with.
  options: ParserOptions<'o, 'i>,
}

/// Options for the `minify` function of a [StyleSheet](StyleSheet)
/// or [StyleAttribute](StyleAttribute).
#[derive(Default)]
pub struct MinifyOptions {
  /// Targets to compile the CSS for.
  pub targets: Targets,
  /// A list of known unused symbols, including CSS class names,
  /// ids, and `@keyframe` names. The declarations of these will be removed.
  pub unused_symbols: HashSet<String>,
}

/// A result returned from `to_css`, including the serialize CSS
/// and other metadata depending on the input options.
#[derive(Debug)]
pub struct ToCssResult {
  /// Serialized CSS code.
  pub code: String,
  /// A map of CSS module exports, if the `css_modules` option was
  /// enabled during parsing.
  pub exports: Option<CssModuleExports>,
  /// A map of CSS module references, if the `css_modules` config
  /// had `dashed_idents` enabled.
  pub references: Option<CssModuleReferences>,
  /// A list of dependencies (e.g. `@import` or `url()`) found in
  /// the style sheet, if the `analyze_dependencies` option is enabled.
  pub dependencies: Option<Vec<Dependency>>,
}

impl<'i, 'o> StyleSheet<'i, 'o, DefaultAtRule> {
  /// Parse a style sheet from a string.
  pub fn parse(code: &'i str, options: ParserOptions<'o, 'i>) -> Result<Self, Error<ParserError<'i>>> {
    Self::parse_with(code, options, &mut DefaultAtRuleParser)
  }
}

impl<'i, 'o, T> StyleSheet<'i, 'o, T>
where
  T: ToCss + Clone,
{
  /// Creates a new style sheet with the given source filenames and rules.
  pub fn new(
    sources: Vec<String>,
    rules: CssRuleList<'i, T>,
    options: ParserOptions<'o, 'i>,
  ) -> StyleSheet<'i, 'o, T> {
    StyleSheet {
      sources,
      source_map_urls: Vec::new(),
      license_comments: Vec::new(),
      content_hashes: None,
      rules,
      options,
    }
  }

  /// Parse a style sheet from a string.
  pub fn parse_with<P: AtRuleParser<'i, AtRule = T>>(
    code: &'i str,
    mut options: ParserOptions<'o, 'i>,
    at_rule_parser: &mut P,
  ) -> Result<Self, Error<ParserError<'i>>> {
    let mut input = ParserInput::new(&code);
    let mut parser = Parser::new(&mut input);
    let mut license_comments = Vec::new();

    let mut content_hashes = None;
    if let Some(config) = &options.css_modules {
      if config.pattern.has_content_hash() {
        content_hashes = Some(vec![hash(
          &code,
          matches!(config.pattern.segments[0], crate::css_modules::Segment::ContentHash),
        )]);
      }
    }

    let mut state = parser.state();
    while let Ok(token) = parser.next_including_whitespace_and_comments() {
      match token {
        cssparser::Token::WhiteSpace(..) => {}
        cssparser::Token::Comment(comment) if comment.starts_with('!') => {
          license_comments.push((*comment).into());
        }
        cssparser::Token::Comment(comment) if comment.contains("cssmodules-pure-no-check") => {
          if let Some(css_modules) = &mut options.css_modules {
            css_modules.pure = false;
          }
        }
        _ => break,
      }
      state = parser.state();
    }
    parser.reset(&state);

    let mut rules = CssRuleList(vec![]);
    let mut rule_parser = TopLevelRuleParser::new(&mut options, at_rule_parser, &mut rules);
    let mut rule_list_parser = StyleSheetParser::new(&mut parser, &mut rule_parser);

    while let Some(rule) = rule_list_parser.next() {
      match rule {
        Ok(()) => {}
        Err((e, _)) => {
          let options = &mut rule_list_parser.parser.options;
          if options.error_recovery {
            options.warn(e);
            continue;
          }

          return Err(Error::from(e, options.filename.clone()));
        }
      }
    }

    Ok(StyleSheet {
      sources: vec![options.filename.clone()],
      source_map_urls: vec![parser.current_source_map_url().map(|s| s.to_owned())],
      content_hashes,
      rules,
      license_comments,
      options,
    })
  }

  /// Returns the source map URL for the source at the given index.
  pub fn source_map_url(&self, source_index: usize) -> Option<&String> {
    self.source_map_urls.get(source_index)?.as_ref()
  }

  /// Returns the inline source map associated with the source at the given index.
  #[cfg(feature = "sourcemap")]
  #[cfg_attr(docsrs, doc(cfg(feature = "sourcemap")))]
  pub fn source_map(&self, source_index: usize) -> Option<SourceMap> {
    SourceMap::from_data_url("/", self.source_map_url(source_index)?).ok()
  }

  /// Minify and transform the style sheet for the provided browser targets.
  pub fn minify(&mut self, options: MinifyOptions) -> Result<(), Error<MinifyErrorKind>> {
    let context = PropertyHandlerContext::new(options.targets, &options.unused_symbols);
    let mut handler = DeclarationHandler::default();
    let mut important_handler = DeclarationHandler::default();

    // @custom-media rules may be defined after they are referenced, but may only be defined at the top level
    // of a stylesheet. Do a pre-scan here and create a lookup table by name.
    let custom_media = if self.options.flags.contains(ParserFlags::CUSTOM_MEDIA)
      && should_compile!(options.targets, CustomMediaQueries)
    {
      let mut custom_media = HashMap::new();
      for rule in &self.rules.0 {
        if let CssRule::CustomMedia(rule) = rule {
          custom_media.insert(rule.name.0.clone(), rule.clone());
        }
      }
      Some(custom_media)
    } else {
      None
    };

    let mut ctx = MinifyContext {
      targets: &options.targets,
      handler: &mut handler,
      important_handler: &mut important_handler,
      handler_context: context,
      unused_symbols: &options.unused_symbols,
      custom_media,
      css_modules: self.options.css_modules.is_some(),
      pure_css_modules: self.options.css_modules.as_ref().map(|c| c.pure).unwrap_or_default(),
    };

    self.rules.minify(&mut ctx, false).map_err(|e| Error {
      kind: e.kind,
      loc: Some(ErrorLocation::new(
        e.loc,
        self.sources[e.loc.source_index as usize].clone(),
      )),
    })?;

    Ok(())
  }

  /// Serialize the style sheet to a CSS string.
  pub fn to_css(&self, options: PrinterOptions) -> Result<ToCssResult, Error<PrinterErrorKind>> {
    // Make sure we always have capacity > 0: https://github.com/napi-rs/napi-rs/issues/1124.
    let mut dest = String::with_capacity(1);
    let project_root = options.project_root.clone();
    let mut printer = Printer::new(&mut dest, options);

    #[cfg(feature = "sourcemap")]
    {
      printer.sources = Some(&self.sources);
    }

    #[cfg(feature = "sourcemap")]
    if printer.source_map.is_some() {
      printer.source_maps = self.sources.iter().enumerate().map(|(i, _)| self.source_map(i)).collect();
    }

    for comment in &self.license_comments {
      printer.write_str("/*")?;
      printer.write_str(comment)?;
      printer.write_str("*/\n")?;
    }

    if let Some(config) = &self.options.css_modules {
      let mut references = HashMap::new();
      printer.css_module = Some(CssModule::new(
        config,
        &self.sources,
        project_root,
        &mut references,
        &self.content_hashes,
      ));

      self.rules.to_css(&mut printer)?;
      printer.newline()?;

      Ok(ToCssResult {
        dependencies: printer.dependencies,
        exports: Some(std::mem::take(
          &mut printer.css_module.unwrap().exports_by_source_index[0],
        )),
        code: dest,
        references: Some(references),
      })
    } else {
      self.rules.to_css(&mut printer)?;
      printer.newline()?;

      Ok(ToCssResult {
        dependencies: printer.dependencies,
        code: dest,
        exports: None,
        references: None,
      })
    }
  }
}

#[cfg(feature = "visitor")]
#[cfg_attr(docsrs, doc(cfg(feature = "visitor")))]
impl<'i, 'o, T, V> Visit<'i, T, V> for StyleSheet<'i, 'o, T>
where
  T: Visit<'i, T, V>,
  V: ?Sized + Visitor<'i, T>,
{
  const CHILD_TYPES: VisitTypes = VisitTypes::all();

  fn visit(&mut self, visitor: &mut V) -> Result<(), V::Error> {
    visitor.visit_stylesheet(self)
  }

  fn visit_children(&mut self, visitor: &mut V) -> Result<(), V::Error> {
    self.rules.visit(visitor)
  }
}

/// An inline style attribute, as in HTML or SVG.
///
/// Style attributes can be parsed from a string, minified and transformed
/// for a set of target browsers, and serialied to a string.
///
/// # Example
///
/// ```
/// use lightningcss::stylesheet::{
///   StyleAttribute, ParserOptions, MinifyOptions, PrinterOptions
/// };
///
/// // Parse a style sheet from a string.
/// let mut style = StyleAttribute::parse(
///   "color: yellow; font-family: 'Helvetica';",
///   ParserOptions::default()
/// ).unwrap();
///
/// // Minify the stylesheet.
/// style.minify(MinifyOptions::default());
///
/// // Serialize it to a string.
/// let res = style.to_css(PrinterOptions::default()).unwrap();
/// assert_eq!(res.code, "color: #ff0; font-family: Helvetica");
/// ```
#[cfg_attr(feature = "visitor", derive(Visit))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct StyleAttribute<'i> {
  /// The declarations in the style attribute.
  pub declarations: DeclarationBlock<'i>,
  #[cfg_attr(feature = "visitor", skip_visit)]
  sources: Vec<String>,
}

impl<'i> StyleAttribute<'i> {
  /// Parses a style attribute from a string.
  pub fn parse(
    code: &'i str,
    options: ParserOptions<'_, 'i>,
  ) -> Result<StyleAttribute<'i>, Error<ParserError<'i>>> {
    let mut input = ParserInput::new(&code);
    let mut parser = Parser::new(&mut input);
    Ok(StyleAttribute {
      declarations: DeclarationBlock::parse(&mut parser, &options).map_err(|e| Error::from(e, "".into()))?,
      sources: vec![options.filename],
    })
  }

  /// Minify and transform the style attribute for the provided browser targets.
  pub fn minify(&mut self, options: MinifyOptions) {
    let mut context = PropertyHandlerContext::new(options.targets, &options.unused_symbols);
    let mut handler = DeclarationHandler::default();
    let mut important_handler = DeclarationHandler::default();
    context.context = DeclarationContext::StyleAttribute;
    self.declarations.minify(&mut handler, &mut important_handler, &mut context);
  }

  /// Serializes the style attribute to a CSS string.
  pub fn to_css(&self, options: PrinterOptions) -> Result<ToCssResult, PrinterError> {
    #[cfg(feature = "sourcemap")]
    assert!(
      options.source_map.is_none(),
      "Source maps are not supported for style attributes"
    );

    // Make sure we always have capacity > 0: https://github.com/napi-rs/napi-rs/issues/1124.
    let mut dest = String::with_capacity(1);
    let mut printer = Printer::new(&mut dest, options);
    printer.sources = Some(&self.sources);

    self.declarations.to_css(&mut printer)?;

    Ok(ToCssResult {
      dependencies: printer.dependencies,
      code: dest,
      exports: None,
      references: None,
    })
  }
}
