use crate::declaration::{parse_declaration, DeclarationBlock, DeclarationList};
use crate::error::{Error, ParserError, PrinterError};
use crate::media_query::*;
use crate::printer::Printer;
use crate::properties::custom::TokenList;
use crate::rules::container::{ContainerCondition, ContainerName, ContainerRule};
use crate::rules::font_palette_values::FontPaletteValuesRule;
use crate::rules::layer::{LayerBlockRule, LayerStatementRule};
use crate::rules::property::PropertyRule;
use crate::rules::scope::ScopeRule;
use crate::rules::viewport::ViewportRule;

use crate::rules::{
  counter_style::CounterStyleRule,
  custom_media::CustomMediaRule,
  document::MozDocumentRule,
  font_face::{FontFaceDeclarationParser, FontFaceRule},
  import::ImportRule,
  keyframes::{KeyframeListParser, KeyframesName, KeyframesRule},
  layer::LayerName,
  media::MediaRule,
  namespace::NamespaceRule,
  nesting::NestingRule,
  page::{PageRule, PageSelector},
  style::StyleRule,
  supports::{SupportsCondition, SupportsRule},
  unknown::UnknownAtRule,
  CssRule, CssRuleList, Location,
};
use crate::selector::{Component, SelectorList, SelectorParser};
use crate::traits::Parse;
use crate::values::ident::{CustomIdent, DashedIdent};
use crate::values::string::CowArcStr;
use crate::vendor_prefix::VendorPrefix;
#[cfg(feature = "visitor")]
use crate::visitor::{Visit, VisitTypes, Visitor};
use cssparser::*;
use parcel_selectors::parser::NestingRequirement;
use std::sync::{Arc, RwLock};

/// CSS parsing options.
#[derive(Clone, Debug, Default)]
pub struct ParserOptions<'o, 'i> {
  /// Filename to use in error messages.
  pub filename: String,
  /// Whether the enable the [CSS nesting](https://www.w3.org/TR/css-nesting-1/) draft syntax.
  pub nesting: bool,
  /// Whether to enable the [custom media](https://drafts.csswg.org/mediaqueries-5/#custom-mq) draft syntax.
  pub custom_media: bool,
  /// Whether to enable the [@scope](https://drafts.csswg.org/css-cascade-6/#scope-atrule) draft syntax.
  pub scope: bool,
  /// Whether the enable [CSS modules](https://github.com/css-modules/css-modules).
  pub css_modules: Option<crate::css_modules::Config<'o>>,
  /// The source index to assign to all parsed rules. Impacts the source map when
  /// the style sheet is serialized.
  pub source_index: u32,
  /// Whether to ignore invalid rules and declarations rather than erroring.
  pub error_recovery: bool,
  /// A list that will be appended to when a warning occurs.
  pub warnings: Option<Arc<RwLock<Vec<Error<ParserError<'i>>>>>>,
}

impl<'o, 'i> ParserOptions<'o, 'i> {
  #[inline]
  pub(crate) fn warn(&self, warning: ParseError<'i, ParserError<'i>>) {
    if let Some(warnings) = &self.warnings {
      if let Ok(mut warnings) = warnings.write() {
        warnings.push(Error::from(warning, self.filename.clone()));
      }
    }
  }
}

#[derive(Clone, Default)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct DefaultAtRuleParser;
impl<'i> crate::traits::AtRuleParser<'i> for DefaultAtRuleParser {
  type AtRule = DefaultAtRule;
  type Error = ();
  type Prelude = ();
}

#[derive(PartialEq, Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
pub struct DefaultAtRule;
impl crate::traits::ToCss for DefaultAtRule {
  fn to_css<W: std::fmt::Write>(&self, _: &mut Printer<W>) -> Result<(), PrinterError> {
    Err(PrinterError {
      kind: crate::error::PrinterErrorKind::FmtError,
      loc: None,
    })
  }
}

#[cfg(feature = "visitor")]
#[cfg_attr(docsrs, doc(cfg(feature = "visitor")))]
impl<'i, V: Visitor<'i, DefaultAtRule>> Visit<'i, DefaultAtRule, V> for DefaultAtRule {
  const CHILD_TYPES: VisitTypes = VisitTypes::empty();
  fn visit_children(&mut self, _: &mut V) -> Result<(), V::Error> {
    Ok(())
  }
}

#[derive(PartialEq, PartialOrd)]
enum State {
  Start = 1,
  Layers = 2,
  Imports = 3,
  Namespaces = 4,
  Body = 5,
}

/// The parser for the top-level rules in a stylesheet.
pub struct TopLevelRuleParser<'a, 'o, 'i, T> {
  pub options: &'a ParserOptions<'o, 'i>,
  state: State,
  at_rule_parser: &'a mut T,
}

impl<'a, 'o, 'b, 'i, T> TopLevelRuleParser<'a, 'o, 'i, T> {
  pub fn new(options: &'a ParserOptions<'o, 'i>, at_rule_parser: &'a mut T) -> Self {
    TopLevelRuleParser {
      options,
      state: State::Start,
      at_rule_parser,
    }
  }

  fn nested<'x: 'b>(&'x mut self) -> NestedRuleParser<'_, 'o, 'i, T> {
    NestedRuleParser {
      options: &self.options,
      at_rule_parser: self.at_rule_parser,
    }
  }
}

/// A rule prelude for at-rule with block.
#[derive(Debug)]
#[allow(dead_code)]
pub enum AtRulePrelude<'i, T> {
  /// A @font-face rule prelude.
  FontFace,

  /// A @scope rule prelude.
  Scope,

  /// A @font-feature-values rule prelude, with its FamilyName list.
  FontFeatureValues, //(Vec<FamilyName>),
  /// A @font-palette-values rule prelude, with its name.
  FontPaletteValues(DashedIdent<'i>),
  /// A @counter-style rule prelude, with its counter style name.
  CounterStyle(CustomIdent<'i>),
  /// A @media rule prelude, with its media queries.
  Media(MediaList<'i>),
  /// A @custom-media rule prelude.
  CustomMedia(DashedIdent<'i>, MediaList<'i>),
  /// An @supports rule, with its conditional
  Supports(SupportsCondition<'i>),
  /// A @viewport rule prelude.
  Viewport(VendorPrefix),
  /// A @keyframes rule, with its animation name and vendor prefix if exists.
  Keyframes(KeyframesName<'i>, VendorPrefix),
  /// A @page rule prelude.
  Page(Vec<PageSelector<'i>>),
  /// A @-moz-document rule.
  MozDocument,
  /// A @import rule prelude.
  Import(
    CowRcStr<'i>,
    MediaList<'i>,
    Option<SupportsCondition<'i>>,
    Option<Option<LayerName<'i>>>,
  ),
  /// A @namespace rule prelude.
  Namespace(Option<CowRcStr<'i>>, CowRcStr<'i>),
  /// A @charset rule prelude.
  Charset,
  /// A @nest prelude.
  Nest(SelectorList<'i>),
  /// An @layer prelude.
  Layer(Vec<LayerName<'i>>),
  /// A @layer block prelude.
  LayerBlock(Option<LayerName<'i>>),
  /// An @property prelude.
  Property(DashedIdent<'i>),
  /// A @container prelude.
  Container(Option<ContainerName<'i>>, ContainerCondition<'i>),
  /// An unknown prelude.
  Unknown(CowArcStr<'i>, TokenList<'i>),
  /// A custom prelude.
  Custom(T),
}

impl<'a, 'o, 'i, T: crate::traits::AtRuleParser<'i>> AtRuleParser<'i> for TopLevelRuleParser<'a, 'o, 'i, T> {
  type Prelude = AtRulePrelude<'i, T::Prelude>;
  type AtRule = (SourcePosition, CssRule<'i, T::AtRule>);
  type Error = ParserError<'i>;

  fn parse_prelude<'t>(
    &mut self,
    name: CowRcStr<'i>,
    input: &mut Parser<'i, 't>,
  ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
    match_ignore_ascii_case! { &*name,
      "import" => {
        if self.state > State::Imports {
          return Err(input.new_custom_error(ParserError::UnexpectedImportRule))
        }

        let url_string = input.expect_url_or_string()?.clone();

        let layer = if input.try_parse(|input| input.expect_ident_matching("layer")).is_ok() {
          Some(None)
        } else if input.try_parse(|input| input.expect_function_matching("layer")).is_ok() {
          let name = input.parse_nested_block(LayerName::parse).map(|name| Some(name))?;
          Some(name)
        } else {
          None
        };

        let supports = if input.try_parse(|input| input.expect_function_matching("supports")).is_ok() {
          Some(input.parse_nested_block(|input| {
            input.try_parse(SupportsCondition::parse).or_else(|_| SupportsCondition::parse_declaration(input))
          })?)
        } else {
          None
        };
        let media = MediaList::parse(input)?;
        return Ok(AtRulePrelude::Import(url_string, media, supports, layer));
      },
      "namespace" => {
        if self.state > State::Namespaces {
          return Err(input.new_custom_error(ParserError::UnexpectedNamespaceRule))
        }

        let prefix = input.try_parse(|input| input.expect_ident_cloned()).ok();
        let namespace = input.expect_url_or_string()?;
        let prelude = AtRulePrelude::Namespace(prefix, namespace);
        return Ok(prelude);
      },
      "charset" => {
        // @charset is removed by rust-cssparser if it’s the first rule in the stylesheet.
        // Anything left is technically invalid, however, users often concatenate CSS files
        // together, so we are more lenient and simply ignore @charset rules in the middle of a file.
        input.expect_string()?;
        return Ok(AtRulePrelude::Charset)
      },
      "custom-media" if self.options.custom_media => {
        let name = DashedIdent::parse(input)?;
        let media = MediaList::parse(input)?;
        return Ok(AtRulePrelude::CustomMedia(name, media))
      },
      "property" => {
        let name = DashedIdent::parse(input)?;
        return Ok(AtRulePrelude::Property(name))
      },
      _ => {}
    }

    AtRuleParser::parse_prelude(&mut self.nested(), name, input)
  }

  #[inline]
  fn parse_block<'t>(
    &mut self,
    prelude: Self::Prelude,
    start: &ParserState,
    input: &mut Parser<'i, 't>,
  ) -> Result<Self::AtRule, ParseError<'i, Self::Error>> {
    self.state = State::Body;
    let rule = AtRuleParser::parse_block(&mut self.nested(), prelude, start, input)?;
    Ok((start.position(), rule))
  }

  #[inline]
  fn rule_without_block(
    &mut self,
    prelude: AtRulePrelude<'i, T::Prelude>,
    start: &ParserState,
  ) -> Result<Self::AtRule, ()> {
    let loc = start.source_location();
    let loc = Location {
      source_index: self.options.source_index,
      line: loc.line,
      column: loc.column,
    };

    let rule = match prelude {
      AtRulePrelude::Import(url, media, supports, layer) => {
        self.state = State::Imports;
        CssRule::Import(ImportRule {
          url: url.into(),
          layer,
          supports,
          media,
          loc,
        })
      }
      AtRulePrelude::Namespace(prefix, url) => {
        self.state = State::Namespaces;

        CssRule::Namespace(NamespaceRule {
          prefix: prefix.map(|x| x.into()),
          url: url.into(),
          loc,
        })
      }
      AtRulePrelude::CustomMedia(name, query) => {
        self.state = State::Body;
        CssRule::CustomMedia(CustomMediaRule { name, query, loc })
      }
      AtRulePrelude::Layer(_) => {
        // @layer statements are allowed before @import rules, but cannot be interleaved.
        if self.state <= State::Layers {
          self.state = State::Layers;
        } else {
          self.state = State::Body;
        }
        AtRuleParser::rule_without_block(&mut self.nested(), prelude, start)?
      }
      AtRulePrelude::Charset => CssRule::Ignored,
      AtRulePrelude::Unknown(name, prelude) => CssRule::Unknown(UnknownAtRule {
        name,
        prelude,
        block: None,
        loc,
      }),
      AtRulePrelude::Custom(_) => {
        self.state = State::Body;
        AtRuleParser::rule_without_block(&mut self.nested(), prelude, start)?
      }
      _ => return Err(()),
    };

    Ok((start.position(), rule))
  }
}

impl<'a, 'o, 'i, T: crate::traits::AtRuleParser<'i>> QualifiedRuleParser<'i>
  for TopLevelRuleParser<'a, 'o, 'i, T>
{
  type Prelude = SelectorList<'i>;
  type QualifiedRule = (SourcePosition, CssRule<'i, T::AtRule>);
  type Error = ParserError<'i>;

  #[inline]
  fn parse_prelude<'t>(
    &mut self,
    input: &mut Parser<'i, 't>,
  ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
    self.state = State::Body;
    QualifiedRuleParser::parse_prelude(&mut self.nested(), input)
  }

  #[inline]
  fn parse_block<'t>(
    &mut self,
    prelude: Self::Prelude,
    start: &ParserState,
    input: &mut Parser<'i, 't>,
  ) -> Result<Self::QualifiedRule, ParseError<'i, Self::Error>> {
    let rule = QualifiedRuleParser::parse_block(&mut self.nested(), prelude, start, input)?;
    Ok((start.position(), rule))
  }
}

pub struct NestedRuleParser<'a, 'o, 'i, T> {
  pub options: &'a ParserOptions<'o, 'i>,
  pub at_rule_parser: &'a mut T,
}

impl<'a, 'o, 'b, 'i, T: crate::traits::AtRuleParser<'i>> NestedRuleParser<'a, 'o, 'i, T> {
  pub fn parse_nested_rules<'t>(
    &mut self,
    input: &mut Parser<'i, 't>,
  ) -> Result<CssRuleList<'i, T::AtRule>, ParseError<'i, ParserError<'i>>> {
    let nested_parser = NestedRuleParser {
      options: self.options,
      at_rule_parser: self.at_rule_parser,
    };

    let mut iter = RuleListParser::new_for_nested_rule(input, nested_parser);
    let mut rules = Vec::new();
    while let Some(result) = iter.next() {
      match result {
        Ok(CssRule::Ignored) => {}
        Ok(rule) => rules.push(rule),
        Err((e, _)) => {
          if iter.parser.options.error_recovery {
            iter.parser.options.warn(e);
            continue;
          }
          return Err(e);
        }
      }
    }

    Ok(CssRuleList(rules))
  }

  fn loc(&self, start: &ParserState) -> Location {
    let loc = start.source_location();
    Location {
      source_index: self.options.source_index,
      line: loc.line,
      column: loc.column,
    }
  }
}

impl<'a, 'o, 'b, 'i, T: crate::traits::AtRuleParser<'i>> AtRuleParser<'i> for NestedRuleParser<'a, 'o, 'i, T> {
  type Prelude = AtRulePrelude<'i, T::Prelude>;
  type AtRule = CssRule<'i, T::AtRule>;
  type Error = ParserError<'i>;

  fn parse_prelude<'t>(
    &mut self,
    name: CowRcStr<'i>,
    input: &mut Parser<'i, 't>,
  ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
    match_ignore_ascii_case! { &*name,
      "media" => {
        let media = MediaList::parse(input)?;
        Ok(AtRulePrelude::Media(media))
      },
      "supports" => {
        let cond = SupportsCondition::parse(input)?;
        Ok(AtRulePrelude::Supports(cond))
      },
      "font-face" => {
        Ok(AtRulePrelude::FontFace)
      },
      "scope" if self.options.scope => {
        Ok(AtRulePrelude::Scope)
      },
      // "font-feature-values" => {
      //     if !cfg!(feature = "gecko") {
      //         // Support for this rule is not fully implemented in Servo yet.
      //         return Err(input.new_custom_error(StyleParseErrorKind::UnsupportedAtRule(name.clone())))
      //     }
      //     let family_names = parse_family_name_list(self.context, input)?;
      //     Ok(AtRuleType::WithBlock(AtRuleBlockPrelude::FontFeatureValues(family_names)))
      // },
      "font-palette-values" => {
        let name = DashedIdent::parse(input)?;
        return Ok(AtRulePrelude::FontPaletteValues(name))
      },
      "counter-style" => {
        let name = CustomIdent::parse(input)?;
        Ok(AtRulePrelude::CounterStyle(name))
      },
      "viewport" | "-ms-viewport" => {
        let prefix = if starts_with_ignore_ascii_case(&*name, "-ms") {
          VendorPrefix::Ms
        } else {
          VendorPrefix::None
        };
        Ok(AtRulePrelude::Viewport(prefix))
      },
      "keyframes" | "-webkit-keyframes" | "-moz-keyframes" | "-o-keyframes" | "-ms-keyframes" => {
        let prefix = if starts_with_ignore_ascii_case(&*name, "-webkit-") {
          VendorPrefix::WebKit
        } else if starts_with_ignore_ascii_case(&*name, "-moz-") {
          VendorPrefix::Moz
        } else if starts_with_ignore_ascii_case(&*name, "-o-") {
          VendorPrefix::O
        } else if starts_with_ignore_ascii_case(&*name, "-ms-") {
          VendorPrefix::Ms
        } else {
          VendorPrefix::None
        };

        let name = input.try_parse(KeyframesName::parse)?;
        Ok(AtRulePrelude::Keyframes(name, prefix))
      },
      "page" => {
        let selectors = input.try_parse(|input| input.parse_comma_separated(PageSelector::parse)).unwrap_or_default();
        Ok(AtRulePrelude::Page(selectors))
      },
      "-moz-document" => {
        // Firefox only supports the url-prefix() function with no arguments as a legacy CSS hack.
        // See https://css-tricks.com/snippets/css/css-hacks-targeting-firefox/
        input.expect_function_matching("url-prefix")?;
        input.parse_nested_block(|input| {
          // Firefox also allows an empty string as an argument...
          // https://github.com/mozilla/gecko-dev/blob/0077f2248712a1b45bf02f0f866449f663538164/servo/components/style/stylesheets/document_rule.rs#L303
          let _ = input.try_parse(|input| -> Result<(), ParseError<'i, Self::Error>> {
            let s = input.expect_string()?;
            if !s.is_empty() {
              return Err(input.new_custom_error(ParserError::InvalidValue))
            }
            Ok(())
          });
          input.expect_exhausted()?;
          Ok(())
        })?;

        Ok(AtRulePrelude::MozDocument)
      },
      "layer" => {
        let names = match Vec::<LayerName>::parse(input) {
          Ok(names) => names,
          Err(ParseError { kind: ParseErrorKind::Basic(BasicParseErrorKind::EndOfInput), .. }) => Vec::new(),
          Err(e) => return Err(e)
        };
        Ok(AtRulePrelude::Layer(names))
      },
      "container" => {
        let name = input.try_parse(ContainerName::parse).ok();
        let condition = ContainerCondition::parse(input)?;
        Ok(AtRulePrelude::Container(name, condition))
      },
      _ => parse_custom_at_rule_prelude(&name, input, self.options, self.at_rule_parser)
    }
  }

  fn parse_block<'t>(
    &mut self,
    prelude: Self::Prelude,
    start: &ParserState,
    input: &mut Parser<'i, 't>,
  ) -> Result<CssRule<'i, T::AtRule>, ParseError<'i, Self::Error>> {
    let loc = self.loc(start);
    match prelude {
      AtRulePrelude::FontFace => {
        let mut parser = DeclarationListParser::new(input, FontFaceDeclarationParser);
        let mut properties = vec![];
        while let Some(decl) = parser.next() {
          if let Ok(decl) = decl {
            properties.push(decl);
          }
        }
        Ok(CssRule::FontFace(FontFaceRule { properties, loc }))
      }
      // AtRuleBlockPrelude::FontFeatureValues(family_names) => {
      //     let context = ParserContext::new_with_rule_type(
      //         self.context,
      //         CssRuleType::FontFeatureValues,
      //         self.namespaces,
      //     );

      //     Ok(CssRule::FontFeatureValues(Arc::new(self.shared_lock.wrap(
      //         FontFeatureValuesRule::parse(
      //             &context,
      //             input,
      //             family_names,
      //             start.source_location(),
      //         ),
      //     ))))
      // },
      AtRulePrelude::FontPaletteValues(name) => {
        let rule = FontPaletteValuesRule::parse(name, input, loc)?;
        Ok(CssRule::FontPaletteValues(rule))
      }
      AtRulePrelude::CounterStyle(name) => Ok(CssRule::CounterStyle(CounterStyleRule {
        name,
        declarations: DeclarationBlock::parse(input, self.options)?,
        loc,
      })),
      AtRulePrelude::Media(query) => Ok(CssRule::Media(MediaRule {
        query,
        rules: self.parse_nested_rules(input)?,
        loc,
      })),
      AtRulePrelude::Supports(condition) => Ok(CssRule::Supports(SupportsRule {
        condition,
        rules: self.parse_nested_rules(input)?,
        loc,
      })),
      AtRulePrelude::Container(name, condition) => Ok(CssRule::Container(ContainerRule {
        name,
        condition,
        rules: self.parse_nested_rules(input)?,
        loc,
      })),
      AtRulePrelude::Scope => Ok(CssRule::Scope(ScopeRule {
        rules: self.parse_nested_rules(input)?,
        loc,
      })),
      AtRulePrelude::Viewport(vendor_prefix) => {
        Ok(CssRule::Viewport(ViewportRule {
          vendor_prefix,
          // TODO: parse viewport descriptors rather than properties
          // https://drafts.csswg.org/css-device-adapt/#viewport-desc
          declarations: DeclarationBlock::parse(input, self.options)?,
          loc,
        }))
      }
      AtRulePrelude::Keyframes(name, vendor_prefix) => {
        let iter = RuleListParser::new_for_nested_rule(input, KeyframeListParser);
        Ok(CssRule::Keyframes(KeyframesRule {
          name,
          keyframes: iter.filter_map(Result::ok).collect(),
          vendor_prefix,
          loc,
        }))
      }
      AtRulePrelude::Page(selectors) => {
        let rule = PageRule::parse(selectors, input, loc, self.options)?;
        Ok(CssRule::Page(rule))
      }
      AtRulePrelude::MozDocument => Ok(CssRule::MozDocument(MozDocumentRule {
        rules: self.parse_nested_rules(input)?,
        loc,
      })),
      AtRulePrelude::Layer(names) => {
        let name = if names.is_empty() {
          None
        } else if names.len() == 1 {
          names.into_iter().next()
        } else {
          return Err(input.new_error(BasicParseErrorKind::AtRuleBodyInvalid));
        };

        Ok(CssRule::LayerBlock(LayerBlockRule {
          name,
          rules: self.parse_nested_rules(input)?,
          loc,
        }))
      }
      AtRulePrelude::LayerBlock(..) => unreachable!(), // only used in nested style rules.
      AtRulePrelude::Property(name) => Ok(CssRule::Property(PropertyRule::parse(name, input, loc)?)),
      AtRulePrelude::Import(..)
      | AtRulePrelude::Namespace(..)
      | AtRulePrelude::CustomMedia(..)
      | AtRulePrelude::Charset => {
        // These rules don't have blocks.
        Err(input.new_unexpected_token_error(Token::CurlyBracketBlock))
      }
      AtRulePrelude::FontFeatureValues | AtRulePrelude::Nest(..) => unreachable!(),
      AtRulePrelude::Unknown(name, prelude) => Ok(CssRule::Unknown(UnknownAtRule {
        name,
        prelude,
        block: Some(TokenList::parse(input, &self.options, 0)?),
        loc,
      })),
      AtRulePrelude::Custom(prelude) => {
        parse_custom_at_rule_body(prelude, input, start, self.options, self.at_rule_parser)
      }
    }
  }

  #[inline]
  fn rule_without_block(
    &mut self,
    prelude: AtRulePrelude<'i, T::Prelude>,
    start: &ParserState,
  ) -> Result<Self::AtRule, ()> {
    let loc = self.loc(start);
    match prelude {
      AtRulePrelude::Layer(names) => {
        if names.is_empty() {
          return Err(());
        }

        Ok(CssRule::LayerStatement(LayerStatementRule { names, loc }))
      }
      AtRulePrelude::Unknown(name, prelude) => Ok(CssRule::Unknown(UnknownAtRule {
        name,
        prelude,
        block: None,
        loc,
      })),
      AtRulePrelude::Custom(prelude) => {
        parse_custom_at_rule_without_block(prelude, start, self.options, self.at_rule_parser)
      }
      _ => Err(()),
    }
  }
}

impl<'a, 'o, 'b, 'i, T: crate::traits::AtRuleParser<'i>> QualifiedRuleParser<'i>
  for NestedRuleParser<'a, 'o, 'i, T>
{
  type Prelude = SelectorList<'i>;
  type QualifiedRule = CssRule<'i, T::AtRule>;
  type Error = ParserError<'i>;

  fn parse_prelude<'t>(
    &mut self,
    input: &mut Parser<'i, 't>,
  ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
    let selector_parser = SelectorParser {
      is_nesting_allowed: self.options.nesting,
      options: &self.options,
    };
    SelectorList::parse(&selector_parser, input, NestingRequirement::None)
  }

  fn parse_block<'t>(
    &mut self,
    selectors: Self::Prelude,
    start: &ParserState,
    input: &mut Parser<'i, 't>,
  ) -> Result<CssRule<'i, T::AtRule>, ParseError<'i, Self::Error>> {
    let loc = self.loc(start);
    let (declarations, rules) = if self.options.nesting {
      parse_declarations_and_nested_rules(input, self.options, self.at_rule_parser)?
    } else {
      (DeclarationBlock::parse(input, self.options)?, CssRuleList(vec![]))
    };
    Ok(CssRule::Style(StyleRule {
      selectors,
      vendor_prefix: VendorPrefix::empty(),
      declarations,
      rules,
      loc,
    }))
  }
}

fn parse_custom_at_rule_prelude<'i, 't, T: crate::traits::AtRuleParser<'i>>(
  name: &CowRcStr<'i>,
  input: &mut Parser<'i, 't>,
  options: &ParserOptions<'_, 'i>,
  at_rule_parser: &mut T,
) -> Result<AtRulePrelude<'i, T::Prelude>, ParseError<'i, ParserError<'i>>> {
  match at_rule_parser.parse_prelude(name.clone(), input, options) {
    Ok(prelude) => return Ok(AtRulePrelude::Custom(prelude)),
    Err(ParseError {
      kind: ParseErrorKind::Basic(BasicParseErrorKind::AtRuleInvalid(..)),
      ..
    }) => {}
    Err(err) => {
      return Err(match &err.kind {
        ParseErrorKind::Basic(kind) => ParseError {
          kind: ParseErrorKind::Basic(kind.clone()),
          location: err.location,
        },
        _ => input.new_custom_error(ParserError::AtRulePreludeInvalid),
      })
    }
  }

  options.warn(input.new_error(BasicParseErrorKind::AtRuleInvalid(name.clone())));
  input.skip_whitespace();
  let tokens = TokenList::parse(input, &options, 0)?;
  Ok(AtRulePrelude::Unknown(name.into(), tokens))
}

fn parse_custom_at_rule_body<'i, 't, T: crate::traits::AtRuleParser<'i>>(
  prelude: T::Prelude,
  input: &mut Parser<'i, 't>,
  start: &ParserState,
  options: &ParserOptions<'_, 'i>,
  at_rule_parser: &mut T,
) -> Result<CssRule<'i, T::AtRule>, ParseError<'i, ParserError<'i>>> {
  at_rule_parser
    .parse_block(prelude, start, input, options)
    .map(|prelude| CssRule::Custom(prelude))
    .map_err(|err| match &err.kind {
      ParseErrorKind::Basic(kind) => ParseError {
        kind: ParseErrorKind::Basic(kind.clone()),
        location: err.location,
      },
      _ => input.new_error(BasicParseErrorKind::AtRuleBodyInvalid),
    })
}

fn parse_custom_at_rule_without_block<'i, 't, T: crate::traits::AtRuleParser<'i>>(
  prelude: T::Prelude,
  start: &ParserState,
  options: &ParserOptions<'_, 'i>,
  at_rule_parser: &mut T,
) -> Result<CssRule<'i, T::AtRule>, ()> {
  at_rule_parser
    .rule_without_block(prelude, start, options)
    .map(|prelude| CssRule::Custom(prelude))
}

fn parse_declarations_and_nested_rules<'a, 'o, 'i, 't, T: crate::traits::AtRuleParser<'i>>(
  input: &mut Parser<'i, 't>,
  options: &'a ParserOptions<'o, 'i>,
  at_rule_parser: &mut T,
) -> Result<(DeclarationBlock<'i>, CssRuleList<'i, T::AtRule>), ParseError<'i, ParserError<'i>>> {
  let mut important_declarations = DeclarationList::new();
  let mut declarations = DeclarationList::new();
  let mut rules = CssRuleList(vec![]);
  let mut parser = StyleRuleParser {
    options,
    declarations: &mut declarations,
    important_declarations: &mut important_declarations,
    rules: &mut rules,
    at_rule_parser,
  };

  // In the v2 nesting spec, declarations and nested rules may be mixed.
  // https://drafts.csswg.org/css-syntax/#consume-style-block
  loop {
    let start = input.state();
    match input.next_including_whitespace_and_comments() {
      Ok(&Token::WhiteSpace(_)) | Ok(&Token::Comment(_)) | Ok(&Token::Semicolon) => continue,
      Ok(&Token::Ident(ref name)) => {
        let name = name.clone();
        let callback = |input: &mut Parser<'i, '_>| {
          input.expect_colon()?;
          parser.parse_value(name, input)
        };
        input.parse_until_after(Delimiter::Semicolon, callback)?;
      }
      Ok(_) => {
        input.reset(&start);
        let mut iter = RuleListParser::new_for_nested_rule(input, parser);
        if let Some(result) = iter.next() {
          if let Err((err, _)) = result {
            if iter.parser.options.error_recovery {
              iter.parser.options.warn(err);
              parser = iter.parser;
              continue;
            }
            return Err(err);
          }
        }
        parser = iter.parser;
      }
      Err(_) => break,
    }
  }

  Ok((
    DeclarationBlock {
      declarations,
      important_declarations,
    },
    rules,
  ))
}

pub struct StyleRuleParser<'a, 'o, 'i, T: crate::traits::AtRuleParser<'i>> {
  options: &'a ParserOptions<'o, 'i>,
  declarations: &'a mut DeclarationList<'i>,
  important_declarations: &'a mut DeclarationList<'i>,
  rules: &'a mut CssRuleList<'i, T::AtRule>,
  at_rule_parser: &'a mut T,
}

/// Parse a declaration within {} block: `color: blue`
impl<'a, 'o, 'i, T: crate::traits::AtRuleParser<'i>> cssparser::DeclarationParser<'i>
  for StyleRuleParser<'a, 'o, 'i, T>
{
  type Declaration = ();
  type Error = ParserError<'i>;

  fn parse_value<'t>(
    &mut self,
    name: CowRcStr<'i>,
    input: &mut cssparser::Parser<'i, 't>,
  ) -> Result<Self::Declaration, cssparser::ParseError<'i, Self::Error>> {
    parse_declaration(
      name,
      input,
      &mut self.declarations,
      &mut self.important_declarations,
      &self.options,
    )
  }
}

impl<'a, 'o, 'i, T: crate::traits::AtRuleParser<'i>> AtRuleParser<'i> for StyleRuleParser<'a, 'o, 'i, T> {
  type Prelude = AtRulePrelude<'i, T::Prelude>;
  type AtRule = ();
  type Error = ParserError<'i>;

  fn parse_prelude<'t>(
    &mut self,
    name: CowRcStr<'i>,
    input: &mut Parser<'i, 't>,
  ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
    match_ignore_ascii_case! { &*name,
      "media" => {
        let media = MediaList::parse(input)?;
        Ok(AtRulePrelude::Media(media))
      },
      "supports" => {
        let cond = SupportsCondition::parse(input)?;
        Ok(AtRulePrelude::Supports(cond))
      },
      "container" => {
        let name = input.try_parse(ContainerName::parse).ok();
        let condition = ContainerCondition::parse(input)?;
        Ok(AtRulePrelude::Container(name, condition))
      },
      "layer" => {
        // Only layer block rules are supported within style rules.
        let name = input.try_parse(LayerName::parse).ok();
        Ok(AtRulePrelude::LayerBlock(name))
      },
      "nest" => {
        self.options.warn(input.new_custom_error(ParserError::DeprecatedNestRule));
        let selector_parser = SelectorParser {
          is_nesting_allowed: true,
          options: &self.options,
        };
        let selectors = SelectorList::parse(&selector_parser, input, NestingRequirement::Contained)?;
        Ok(AtRulePrelude::Nest(selectors))
      },
      _ => parse_custom_at_rule_prelude(&name, input, self.options, self.at_rule_parser)
    }
  }

  fn parse_block<'t>(
    &mut self,
    prelude: AtRulePrelude<'i, T::Prelude>,
    start: &ParserState,
    input: &mut Parser<'i, 't>,
  ) -> Result<(), ParseError<'i, Self::Error>> {
    let loc = start.source_location();
    let loc = Location {
      source_index: self.options.source_index,
      line: loc.line,
      column: loc.column,
    };
    match prelude {
      AtRulePrelude::Media(query) => {
        self.rules.0.push(CssRule::Media(MediaRule {
          query,
          rules: parse_nested_at_rule(input, self.options, self.at_rule_parser)?,
          loc,
        }));
        Ok(())
      }
      AtRulePrelude::Supports(condition) => {
        self.rules.0.push(CssRule::Supports(SupportsRule {
          condition,
          rules: parse_nested_at_rule(input, self.options, self.at_rule_parser)?,
          loc,
        }));
        Ok(())
      }
      AtRulePrelude::Container(name, condition) => {
        self.rules.0.push(CssRule::Container(ContainerRule {
          name,
          condition,
          rules: parse_nested_at_rule(input, self.options, self.at_rule_parser)?,
          loc,
        }));
        Ok(())
      }
      AtRulePrelude::LayerBlock(name) => {
        self.rules.0.push(CssRule::LayerBlock(LayerBlockRule {
          name,
          rules: parse_nested_at_rule(input, self.options, self.at_rule_parser)?,
          loc,
        }));
        Ok(())
      }
      AtRulePrelude::Nest(selectors) => {
        let (declarations, rules) = parse_declarations_and_nested_rules(input, self.options, self.at_rule_parser)?;
        self.rules.0.push(CssRule::Nesting(NestingRule {
          style: StyleRule {
            selectors,
            declarations,
            vendor_prefix: VendorPrefix::empty(),
            rules,
            loc,
          },
          loc,
        }));
        Ok(())
      }
      AtRulePrelude::Unknown(name, prelude) => {
        self.rules.0.push(CssRule::Unknown(UnknownAtRule {
          name,
          prelude,
          block: Some(TokenList::parse(input, &self.options, 0)?),
          loc,
        }));
        Ok(())
      }
      AtRulePrelude::Custom(prelude) => {
        self.rules.0.push(parse_custom_at_rule_body(
          prelude,
          input,
          start,
          self.options,
          self.at_rule_parser,
        )?);
        Ok(())
      }
      _ => Err(input.new_error(BasicParseErrorKind::AtRuleBodyInvalid)),
    }
  }

  fn rule_without_block(&mut self, prelude: Self::Prelude, start: &ParserState) -> Result<Self::AtRule, ()> {
    match prelude {
      AtRulePrelude::Unknown(name, prelude) => {
        let loc = start.source_location();
        self.rules.0.push(CssRule::Unknown(UnknownAtRule {
          name,
          prelude,
          block: None,
          loc: Location {
            source_index: self.options.source_index,
            line: loc.line,
            column: loc.column,
          },
        }));
        Ok(())
      }
      AtRulePrelude::Custom(prelude) => {
        self.rules.0.push(parse_custom_at_rule_without_block(
          prelude,
          start,
          self.options,
          self.at_rule_parser,
        )?);
        Ok(())
      }
      _ => Err(()),
    }
  }
}

pub fn parse_nested_at_rule<'a, 'o, 'i, 't, T: crate::traits::AtRuleParser<'i>>(
  input: &mut Parser<'i, 't>,
  options: &'a ParserOptions<'o, 'i>,
  at_rule_parser: &mut T,
) -> Result<CssRuleList<'i, T::AtRule>, ParseError<'i, ParserError<'i>>> {
  let loc = input.current_source_location();
  let loc = Location {
    source_index: options.source_index,
    line: loc.line,
    column: loc.column,
  };

  // Declarations can be immediately within @media and @supports blocks that are nested within a parent style rule.
  // These act the same way as if they were nested within a `& { ... }` block.
  let (declarations, mut rules) = parse_declarations_and_nested_rules(input, options, at_rule_parser)?;

  if declarations.len() > 0 {
    rules.0.insert(
      0,
      CssRule::Style(StyleRule {
        selectors: Component::Nesting.into(),
        declarations,
        vendor_prefix: VendorPrefix::empty(),
        rules: CssRuleList(vec![]),
        loc,
      }),
    )
  }

  Ok(rules)
}

impl<'a, 'o, 'b, 'i, T: crate::traits::AtRuleParser<'i>> QualifiedRuleParser<'i>
  for StyleRuleParser<'a, 'o, 'i, T>
{
  type Prelude = SelectorList<'i>;
  type QualifiedRule = ();
  type Error = ParserError<'i>;

  fn parse_prelude<'t>(
    &mut self,
    input: &mut Parser<'i, 't>,
  ) -> Result<Self::Prelude, ParseError<'i, Self::Error>> {
    let selector_parser = SelectorParser {
      is_nesting_allowed: true,
      options: &self.options,
    };
    SelectorList::parse_relative(&selector_parser, input, NestingRequirement::Implicit)
  }

  fn parse_block<'t>(
    &mut self,
    selectors: Self::Prelude,
    start: &ParserState,
    input: &mut Parser<'i, 't>,
  ) -> Result<(), ParseError<'i, Self::Error>> {
    let loc = start.source_location();
    let (declarations, rules) = parse_declarations_and_nested_rules(input, self.options, self.at_rule_parser)?;
    self.rules.0.push(CssRule::Style(StyleRule {
      selectors,
      vendor_prefix: VendorPrefix::empty(),
      declarations,
      rules,
      loc: Location {
        source_index: self.options.source_index,
        line: loc.line,
        column: loc.column,
      },
    }));
    Ok(())
  }
}

#[inline]
pub fn starts_with_ignore_ascii_case(string: &str, prefix: &str) -> bool {
  string.len() >= prefix.len() && string.as_bytes()[0..prefix.len()].eq_ignore_ascii_case(prefix.as_bytes())
}
