//! Lightning CSS is a CSS parser, transformer, and minifier based on the
//! [cssparser](https://github.com/servo/rust-cssparser) crate used in Firefox.
//! It supports fully parsing all CSS rules, properties, and values into normalized
//! structures exactly how a browser would. Once parsed, the CSS can be transformed
//! to add or remove vendor prefixes, or lower syntax for older browsers as appropriate.
//! The style sheet can also be minified to merge longhand properties into shorthands,
//! merge adjacent rules, reduce `calc()` expressions, and more. Finally, the style sheet
//! can be printed back to CSS syntax, either minified to remove whitespace and compress
//! the output as much as possible, or pretty printed.
//!
//! The [StyleSheet](stylesheet::StyleSheet) struct is the main entrypoint for Lightning CSS,
//! and supports parsing and transforming entire CSS files. You can also parse and manipulate
//! individual CSS [rules](rules), [properties](properties), or [values](values). The [bundler](bundler)
//! module also can be used to combine a CSS file and all of its dependencies together into a single
//! style sheet. See the individual module documentation for more details and examples.

#![deny(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "bundler")]
#[cfg_attr(docsrs, doc(cfg(feature = "bundler")))]
pub mod bundler;
mod compat;
mod context;
pub mod css_modules;
pub mod declaration;
pub mod dependencies;
pub mod error;
mod logical;
mod macros;
pub mod media_query;
mod parser;
mod prefixes;
pub mod printer;
pub mod properties;
pub mod rules;
pub mod selector;
pub mod stylesheet;
pub mod targets;
pub mod traits;
pub mod values;
pub mod vendor_prefix;
#[cfg(feature = "visitor")]
#[cfg_attr(docsrs, doc(cfg(feature = "visitor")))]
pub mod visitor;

#[cfg(feature = "serde")]
mod serialization;

#[cfg(test)]
mod tests {
  use crate::css_modules::{CssModuleExport, CssModuleExports, CssModuleReference, CssModuleReferences};
  use crate::dependencies::Dependency;
  use crate::error::{Error, ErrorLocation, MinifyErrorKind, ParserError, PrinterErrorKind, SelectorError};
  use crate::parser::ParserFlags;
  use crate::properties::custom::Token;
  use crate::properties::Property;
  use crate::rules::CssRule;
  use crate::rules::Location;
  use crate::stylesheet::*;
  use crate::targets::{Browsers, Features, Targets};
  use crate::traits::{Parse, ToCss};
  use crate::values::color::CssColor;
  use crate::vendor_prefix::VendorPrefix;
  use cssparser::SourceLocation;
  use indoc::indoc;
  use pretty_assertions::assert_eq;
  use std::collections::HashMap;

  fn test(source: &str, expected: &str) {
    test_with_options(source, expected, ParserOptions::default())
  }

  fn test_with_options<'i, 'o>(source: &'i str, expected: &'i str, options: ParserOptions<'o, 'i>) {
    let mut stylesheet = StyleSheet::parse(&source, options).unwrap();
    stylesheet.minify(MinifyOptions::default()).unwrap();
    let res = stylesheet.to_css(PrinterOptions::default()).unwrap();
    assert_eq!(res.code, expected);
  }

  fn minify_test(source: &str, expected: &str) {
    minify_test_with_options(source, expected, ParserOptions::default())
  }

  #[track_caller]
  fn minify_test_with_options<'i, 'o>(source: &'i str, expected: &'i str, options: ParserOptions<'o, 'i>) {
    let mut stylesheet = StyleSheet::parse(&source, options.clone()).unwrap();
    stylesheet.minify(MinifyOptions::default()).unwrap();
    let res = stylesheet
      .to_css(PrinterOptions {
        minify: true,
        ..PrinterOptions::default()
      })
      .unwrap();
    assert_eq!(res.code, expected);
  }

  fn minify_error_test_with_options<'i, 'o>(
    source: &'i str,
    error: MinifyErrorKind,
    options: ParserOptions<'o, 'i>,
  ) {
    let mut stylesheet = StyleSheet::parse(&source, options.clone()).unwrap();
    match stylesheet.minify(MinifyOptions::default()) {
      Err(e) => assert_eq!(e.kind, error),
      _ => unreachable!(),
    }
  }

  fn prefix_test(source: &str, expected: &str, targets: Browsers) {
    let mut stylesheet = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
    stylesheet
      .minify(MinifyOptions {
        targets: targets.into(),
        ..MinifyOptions::default()
      })
      .unwrap();
    let res = stylesheet
      .to_css(PrinterOptions {
        targets: targets.into(),
        ..PrinterOptions::default()
      })
      .unwrap();
    assert_eq!(res.code, expected);
  }

  fn attr_test(source: &str, expected: &str, minify: bool, targets: Option<Browsers>) {
    let mut attr = StyleAttribute::parse(source, ParserOptions::default()).unwrap();
    attr.minify(MinifyOptions {
      targets: targets.into(),
      ..MinifyOptions::default()
    });
    let res = attr
      .to_css(PrinterOptions {
        targets: targets.into(),
        minify,
        ..PrinterOptions::default()
      })
      .unwrap();
    assert_eq!(res.code, expected);
  }

  fn nesting_test(source: &str, expected: &str) {
    nesting_test_with_targets(
      source,
      expected,
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      }
      .into(),
    );
  }

  fn nesting_test_with_targets(source: &str, expected: &str, targets: Targets) {
    let mut stylesheet = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
    stylesheet
      .minify(MinifyOptions {
        targets,
        ..MinifyOptions::default()
      })
      .unwrap();
    let res = stylesheet
      .to_css(PrinterOptions {
        targets,
        ..PrinterOptions::default()
      })
      .unwrap();
    assert_eq!(res.code, expected);
  }

  fn nesting_test_no_targets(source: &str, expected: &str) {
    let mut stylesheet = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
    stylesheet.minify(MinifyOptions::default()).unwrap();
    let res = stylesheet.to_css(PrinterOptions::default()).unwrap();
    assert_eq!(res.code, expected);
  }

  fn css_modules_test<'i>(
    source: &'i str,
    expected: &str,
    expected_exports: CssModuleExports,
    expected_references: CssModuleReferences,
    config: crate::css_modules::Config<'i>,
    minify: bool,
  ) {
    let mut stylesheet = StyleSheet::parse(
      &source,
      ParserOptions {
        filename: "test.css".into(),
        css_modules: Some(config),
        ..ParserOptions::default()
      },
    )
    .unwrap();
    stylesheet.minify(MinifyOptions::default()).unwrap();
    let res = stylesheet
      .to_css(PrinterOptions {
        minify,
        ..Default::default()
      })
      .unwrap();
    assert_eq!(res.code, expected);
    assert_eq!(res.exports.unwrap(), expected_exports);
    assert_eq!(res.references.unwrap(), expected_references);
  }

  fn custom_media_test(source: &str, expected: &str) {
    let mut stylesheet = StyleSheet::parse(
      &source,
      ParserOptions {
        flags: ParserFlags::CUSTOM_MEDIA,
        ..ParserOptions::default()
      },
    )
    .unwrap();
    stylesheet
      .minify(MinifyOptions {
        targets: Browsers {
          chrome: Some(95 << 16),
          ..Browsers::default()
        }
        .into(),
        ..MinifyOptions::default()
      })
      .unwrap();
    let res = stylesheet.to_css(PrinterOptions::default()).unwrap();
    assert_eq!(res.code, expected);
  }

  fn error_test(source: &str, error: ParserError) {
    let res = StyleSheet::parse(&source, ParserOptions::default());
    match res {
      Ok(_) => unreachable!(),
      Err(e) => assert_eq!(e.kind, error),
    }
  }

  fn css_modules_error_test(source: &str, error: ParserError) {
    let res = StyleSheet::parse(
      &source,
      ParserOptions {
        css_modules: Some(Default::default()),
        ..Default::default()
      },
    );
    match res {
      Ok(_) => unreachable!(),
      Err(e) => assert_eq!(e.kind, error),
    }
  }

  macro_rules! map(
    { $($key:expr => $name:literal $(referenced: $referenced: literal)? $($value:literal $(global: $global: literal)? $(from $from:literal)?)*),* } => {
      {
        #[allow(unused_mut)]
        let mut m = HashMap::new();
        $(
          #[allow(unused_mut)]
          let mut v = Vec::new();
          #[allow(unused_macros)]
          macro_rules! insert {
            ($local:literal from $specifier:literal) => {
              v.push(CssModuleReference::Dependency {
                name: $local.into(),
                specifier: $specifier.into()
              });
            };
            ($local:literal global: $is_global: literal) => {
              v.push(CssModuleReference::Global {
                name: $local.into()
              });
            };
            ($local:literal) => {
              v.push(CssModuleReference::Local {
                name: $local.into()
              });
            };
          }
          $(
            insert!($value $(global: $global)? $(from $from)?);
          )*

          macro_rules! is_referenced {
            ($ref: literal) => {
              $ref
            };
            () => {
              false
            };
          }

          m.insert($key.into(), CssModuleExport {
            name: $name.into(),
            composes: v,
            is_referenced: is_referenced!($($referenced)?)
          });
        )*
        m
      }
    };
  );

  #[test]
  pub fn test_border_spacing() {
    minify_test(
      r#"
      .foo {
        border-spacing: 0px;
      }
    "#,
      indoc! {".foo{border-spacing:0}"
      },
    );
    minify_test(
      r#"
      .foo {
        border-spacing: 0px 0px;
      }
    "#,
      indoc! {".foo{border-spacing:0}"
      },
    );

    minify_test(
      r#"
      .foo {
        border-spacing: 12px   0px;
      }
    "#,
      indoc! {".foo{border-spacing:12px 0}"
      },
    );

    minify_test(
      r#"
      .foo {
        border-spacing: calc(3px * 2) calc(5px * 0);
      }
    "#,
      indoc! {".foo{border-spacing:6px 0}"
      },
    );

    minify_test(
      r#"
      .foo {
        border-spacing: calc(3px * 2) max(0px, 8px);
      }
    "#,
      indoc! {".foo{border-spacing:6px 8px}"
      },
    );

    // TODO: The `<length>` in border-spacing cannot have a negative value,
    // we may need to implement NonNegativeLength like Servo does.
    // Servo Code: https://github.com/servo/servo/blob/08bc2d53579c9ab85415d4363888881b91df073b/components/style/values/specified/length.rs#L875
    // CSSWG issue: https://lists.w3.org/Archives/Public/www-style/2008Sep/0161.html
    // `border-spacing = <length> <length>?`
    minify_test(
      r#"
      .foo {
        border-spacing: -20px;
      }
    "#,
      indoc! {".foo{border-spacing:-20px}"
      },
    );
  }

  #[test]
  pub fn test_border() {
    test(
      r#"
      .foo {
        border-left: 2px solid red;
        border-right: 2px solid red;
        border-bottom: 2px solid red;
        border-top: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 2px solid red;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-left-color: red;
        border-right-color: red;
        border-bottom-color: red;
        border-top-color: red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-color: red;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-left-width: thin;
        border-right-width: thin;
        border-bottom-width: thin;
        border-top-width: thin;
      }
    "#,
      indoc! {r#"
      .foo {
        border-width: thin;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-left-style: dotted;
        border-right-style: dotted;
        border-bottom-style: dotted;
        border-top-style: dotted;
      }
    "#,
      indoc! {r#"
      .foo {
        border-style: dotted;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-left-width: thin;
        border-left-style: dotted;
        border-left-color: red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-left: thin dotted red;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-left-width: thick;
        border-left: thin dotted red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-left: thin dotted red;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-left-width: thick;
        border: thin dotted red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: thin dotted red;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border: thin dotted red;
        border-right-width: thick;
      }
    "#,
      indoc! {r#"
      .foo {
        border: thin dotted red;
        border-right-width: thick;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border: thin dotted red;
        border-right: thick dotted red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: thin dotted red;
        border-right-width: thick;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border: thin dotted red;
        border-right-width: thick;
        border-right-style: solid;
      }
    "#,
      indoc! {r#"
      .foo {
        border: thin dotted red;
        border-right: thick solid red;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-top: thin dotted red;
        border-block-start: thick solid green;
      }
    "#,
      indoc! {r#"
      .foo {
        border-top: thin dotted red;
        border-block-start: thick solid green;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border: thin dotted red;
        border-block-start-width: thick;
        border-left-width: medium;
      }
    "#,
      indoc! {r#"
      .foo {
        border: thin dotted red;
        border-block-start-width: thick;
        border-left-width: medium;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-block-start: thin dotted red;
        border-inline-end: thin dotted red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-block-start: thin dotted red;
        border-inline-end: thin dotted red;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-block-start-width: thin;
        border-block-start-style: dotted;
        border-block-start-color: red;
        border-inline-end: thin dotted red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-block-start: thin dotted red;
        border-inline-end: thin dotted red;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-block-start: thin dotted red;
        border-block-end: thin dotted red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-block: thin dotted red;
      }
    "#
      },
    );

    minify_test(
      r#"
      .foo {
        border: none;
      }
    "#,
      indoc! {".foo{border:none}"
      },
    );

    minify_test(".foo { border-width: 0 0 1px; }", ".foo{border-width:0 0 1px}");
    test(
      r#"
      .foo {
        border-block-width: 1px;
        border-inline-width: 1px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-width: 1px;
      }
    "#
      },
    );
    test(
      r#"
      .foo {
        border-block-start-width: 1px;
        border-block-end-width: 1px;
        border-inline-start-width: 1px;
        border-inline-end-width: 1px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-width: 1px;
      }
    "#
      },
    );
    test(
      r#"
      .foo {
        border-block-start-width: 1px;
        border-block-end-width: 1px;
        border-inline-start-width: 2px;
        border-inline-end-width: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-block-width: 1px;
        border-inline-width: 2px;
      }
    "#
      },
    );
    test(
      r#"
      .foo {
        border-block-start-width: 1px;
        border-block-end-width: 1px;
        border-inline-start-width: 2px;
        border-inline-end-width: 3px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-block-width: 1px;
        border-inline-width: 2px 3px;
      }
    "#
      },
    );

    minify_test(
      ".foo { border-bottom: 1px solid var(--spectrum-global-color-gray-200)}",
      ".foo{border-bottom:1px solid var(--spectrum-global-color-gray-200)}",
    );
    test(
      r#"
      .foo {
        border-width: 0;
        border-bottom: var(--test, 1px) solid;
      }
    "#,
      indoc! {r#"
      .foo {
        border-width: 0;
        border-bottom: var(--test, 1px) solid;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border: 1px solid black;
        border-width: 1px 1px 0 0;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid #000;
        border-width: 1px 1px 0 0;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border-top: 1px solid black;
        border-bottom: 1px solid black;
        border-left: 2px solid black;
        border-right: 2px solid black;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid #000;
        border-width: 1px 2px;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border-top: 1px solid black;
        border-bottom: 1px solid black;
        border-left: 2px solid black;
        border-right: 1px solid black;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid #000;
        border-left-width: 2px;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border-top: 1px solid black;
        border-bottom: 1px solid black;
        border-left: 1px solid red;
        border-right: 1px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid #000;
        border-color: #000 red;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border-block-start: 1px solid black;
        border-block-end: 1px solid black;
        border-inline-start: 1px solid red;
        border-inline-end: 1px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid #000;
        border-inline-color: red;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border-block-start: 1px solid black;
        border-block-end: 1px solid black;
        border-inline-start: 2px solid black;
        border-inline-end: 2px solid black;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid #000;
        border-inline-width: 2px;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border-block-start: 1px solid black;
        border-block-end: 1px solid black;
        border-inline-start: 2px solid red;
        border-inline-end: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid #000;
        border-inline: 2px solid red;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border-block-start: 1px solid black;
        border-block-end: 1px solid black;
        border-inline-start: 2px solid red;
        border-inline-end: 3px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid #000;
        border-inline-start: 2px solid red;
        border-inline-end: 3px solid red;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border-block-start: 2px solid black;
        border-block-end: 1px solid black;
        border-inline-start: 2px solid red;
        border-inline-end: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 2px solid red;
        border-block-start-color: #000;
        border-block-end: 1px solid #000;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border-block-start: 2px solid red;
        border-block-end: 1px solid red;
        border-inline-start: 2px solid red;
        border-inline-end: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 2px solid red;
        border-block-end-width: 1px;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border-block-start: 2px solid red;
        border-block-end: 2px solid red;
        border-inline-start: 2px solid red;
        border-inline-end: 1px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 2px solid red;
        border-inline-end-width: 1px;
      }
    "#},
    );

    test(
      r#"
      .foo {
        border: 1px solid currentColor;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid;
      }
    "#
      },
    );

    minify_test(
      r#"
      .foo {
        border: 1px solid currentColor;
      }
    "#,
      ".foo{border:1px solid}",
    );

    prefix_test(
      r#"
      .foo {
        border-block: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-top: 2px solid red;
        border-bottom: 2px solid red;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-block-start: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-top: 2px solid red;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-block-end: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-bottom: 2px solid red;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-left: 2px solid red;
        border-right: 2px solid red;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-block-width: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-block-start-width: 2px;
        border-block-end-width: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-block-width: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-block-width: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-start: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: 2px solid red;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: 2px solid red;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-right: 2px solid red;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-right: 2px solid red;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-start-width: 2px;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left-width: 2px;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left-width: 2px;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-right-width: 2px;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-right-width: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-end: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-right: 2px solid red;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-right: 2px solid red;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: 2px solid red;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: 2px solid red;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-start: 2px solid red;
        border-inline-end: 5px solid green;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: 2px solid red;
        border-right: 5px solid green;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: 2px solid red;
        border-right: 5px solid green;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: 5px solid green;
        border-right: 2px solid red;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: 5px solid green;
        border-right: 2px solid red;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-start: 2px solid red;
        border-inline-end: 5px solid green;
      }

      .bar {
        border-inline-start: 1px dotted gray;
        border-inline-end: 1px solid black;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: 2px solid red;
        border-right: 5px solid green;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: 2px solid red;
        border-right: 5px solid green;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: 5px solid green;
        border-right: 2px solid red;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: 5px solid green;
        border-right: 2px solid red;
      }

      .bar:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: 1px dotted gray;
        border-right: 1px solid #000;
      }

      .bar:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: 1px dotted gray;
        border-right: 1px solid #000;
      }

      .bar:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: 1px solid #000;
        border-right: 1px dotted gray;
      }

      .bar:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: 1px solid #000;
        border-right: 1px dotted gray;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-width: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-left-width: 2px;
        border-right-width: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-width: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-left-width: 2px;
        border-right-width: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-style: solid;
      }
    "#,
      indoc! {r#"
      .foo {
        border-left-style: solid;
        border-right-style: solid;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-color: red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-left-color: red;
        border-right-color: red;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-end: var(--test);
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-right: var(--test);
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-right: var(--test);
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: var(--test);
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: var(--test);
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-start: var(--start);
        border-inline-end: var(--end);
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: var(--start);
        border-right: var(--end);
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: var(--start);
        border-right: var(--end);
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-right: var(--start);
        border-left: var(--end);
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-right: var(--start);
        border-left: var(--end);
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    for prop in &[
      "border-inline-start-color",
      "border-inline-end-color",
      "border-block-start-color",
      "border-block-end-color",
      "border-top-color",
      "border-bottom-color",
      "border-left-color",
      "border-right-color",
      "border-color",
      "border-block-color",
      "border-inline-color",
    ] {
      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: lab(40% 56.6 39);
        }}
      "#,
          prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: #b32323;
          {}: lab(40% 56.6 39);
        }}
      "#},
          prop, prop
        ),
        Browsers {
          chrome: Some(90 << 16),
          ..Browsers::default()
        },
      );
    }

    for prop in &[
      "border",
      "border-inline",
      "border-block",
      "border-left",
      "border-right",
      "border-top",
      "border-bottom",
      "border-block-start",
      "border-block-end",
      "border-inline-start",
      "border-inline-end",
    ] {
      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: 2px solid lab(40% 56.6 39);
        }}
      "#,
          prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: 2px solid #b32323;
          {}: 2px solid lab(40% 56.6 39);
        }}
      "#},
          prop, prop
        ),
        Browsers {
          chrome: Some(90 << 16),
          ..Browsers::default()
        },
      );
    }

    for prop in &[
      "border",
      "border-inline",
      "border-block",
      "border-left",
      "border-right",
      "border-top",
      "border-bottom",
      "border-block-start",
      "border-block-end",
      "border-inline-start",
      "border-inline-end",
    ] {
      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: var(--border-width) solid lab(40% 56.6 39);
        }}
      "#,
          prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: var(--border-width) solid #b32323;
        }}

        @supports (color: lab(0% 0 0)) {{
          .foo {{
            {}: var(--border-width) solid lab(40% 56.6 39);
          }}
        }}
      "#},
          prop, prop
        ),
        Browsers {
          chrome: Some(90 << 16),
          ..Browsers::default()
        },
      );
    }

    prefix_test(
      r#"
      .foo {
        border-inline-start-color: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left-color: #b32323;
        border-left-color: lab(40% 56.6 39);
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left-color: #b32323;
        border-left-color: lab(40% 56.6 39);
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-right-color: #b32323;
        border-right-color: lab(40% 56.6 39);
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-right-color: #b32323;
        border-right-color: lab(40% 56.6 39);
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-end-color: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-right-color: #b32323;
        border-right-color: lab(40% 56.6 39);
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-right-color: #b32323;
        border-right-color: lab(40% 56.6 39);
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left-color: #b32323;
        border-left-color: lab(40% 56.6 39);
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left-color: #b32323;
        border-left-color: lab(40% 56.6 39);
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-start-color: lab(40% 56.6 39);
        border-inline-end-color: lch(50.998% 135.363 338);
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left-color: #b32323;
        border-left-color: lab(40% 56.6 39);
        border-right-color: #ee00be;
        border-right-color: lch(50.998% 135.363 338);
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left-color: #b32323;
        border-left-color: lab(40% 56.6 39);
        border-right-color: #ee00be;
        border-right-color: lch(50.998% 135.363 338);
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left-color: #ee00be;
        border-left-color: lch(50.998% 135.363 338);
        border-right-color: #b32323;
        border-right-color: lab(40% 56.6 39);
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left-color: #ee00be;
        border-left-color: lch(50.998% 135.363 338);
        border-right-color: #b32323;
        border-right-color: lab(40% 56.6 39);
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-start-color: lab(40% 56.6 39);
        border-inline-end-color: lch(50.998% 135.363 338);
      }
    "#,
      indoc! {r#"
      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left-color: #b32323;
        border-left-color: color(display-p3 .643308 .192455 .167712);
        border-left-color: lab(40% 56.6 39);
        border-right-color: #ee00be;
        border-right-color: color(display-p3 .972962 -.362078 .804206);
        border-right-color: lch(50.998% 135.363 338);
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left-color: #ee00be;
        border-left-color: color(display-p3 .972962 -.362078 .804206);
        border-left-color: lch(50.998% 135.363 338);
        border-right-color: #b32323;
        border-right-color: color(display-p3 .643308 .192455 .167712);
        border-right-color: lab(40% 56.6 39);
      }
    "#},
      Browsers {
        chrome: Some(8 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-start: 2px solid lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: 2px solid #b32323;
        border-left: 2px solid lab(40% 56.6 39);
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-left: 2px solid #b32323;
        border-left: 2px solid lab(40% 56.6 39);
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-right: 2px solid #b32323;
        border-right: 2px solid lab(40% 56.6 39);
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-right: 2px solid #b32323;
        border-right: 2px solid lab(40% 56.6 39);
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-end: 2px solid lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-right: 2px solid #b32323;
        border-right: 2px solid lab(40% 56.6 39);
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-right: 2px solid #b32323;
        border-right: 2px solid lab(40% 56.6 39);
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: 2px solid #b32323;
        border-left: 2px solid lab(40% 56.6 39);
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: 2px solid #b32323;
        border-left: 2px solid lab(40% 56.6 39);
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-end: var(--border-width) solid lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-right: var(--border-width) solid #b32323;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        border-right: var(--border-width) solid #b32323;
      }

      @supports (color: lab(0% 0 0)) {
        .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
          border-right: var(--border-width) solid lab(40% 56.6 39);
        }
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: var(--border-width) solid #b32323;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        border-left: var(--border-width) solid #b32323;
      }

      @supports (color: lab(0% 0 0)) {
        .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
          border-left: var(--border-width) solid lab(40% 56.6 39);
        }
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-start: 2px solid red;
        border-inline-end: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-inline-start: 2px solid red;
        border-inline-end: 2px solid red;
      }
    "#
      },
      Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-inline-start: 2px solid red;
        border-inline-end: 2px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border-inline: 2px solid red;
      }
    "#
      },
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-width: 22px;
        border-width: max(2cqw, 22px);
      }
    "#,
      indoc! {r#"
      .foo {
        border-width: 22px;
        border-width: max(2cqw, 22px);
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        border-width: 22px;
        border-width: max(2cqw, 22px);
      }
    "#,
      indoc! {r#"
      .foo {
        border-width: max(2cqw, 22px);
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        border-color: #4263eb;
        border-color: color(display-p3 0 .5 1);
      }
    "#,
      indoc! {r#"
      .foo {
        border-color: #4263eb;
        border-color: color(display-p3 0 .5 1);
      }
    "#
      },
      Browsers {
        chrome: Some(99 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        border-color: #4263eb;
        border-color: color(display-p3 0 .5 1);
      }
    "#,
      indoc! {r#"
      .foo {
        border-color: color(display-p3 0 .5 1);
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        border: 1px solid #4263eb;
        border-color: color(display-p3 0 .5 1);
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid #4263eb;
        border-color: color(display-p3 0 .5 1);
      }
    "#
      },
      Browsers {
        chrome: Some(99 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        border: 1px solid #4263eb;
        border-color: color(display-p3 0 .5 1);
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid color(display-p3 0 .5 1);
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        border-color: var(--fallback);
        border-color: color(display-p3 0 .5 1);
      }
    "#,
      indoc! {r#"
      .foo {
        border-color: var(--fallback);
        border-color: color(display-p3 0 .5 1);
      }
    "#
      },
      Browsers {
        chrome: Some(99 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  pub fn test_border_image() {
    test(
      r#"
      .foo {
        border-image: url(test.png) 60;
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: url("test.png") 60;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-image: url(test.png) 60;
        border-image-source: url(foo.png);
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: url("foo.png") 60;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-image-source: url(foo.png);
        border-image-slice: 10 40 10 40 fill;
        border-image-width: 10px;
        border-image-outset: 0;
        border-image-repeat: round round;
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: url("foo.png") 10 40 fill / 10px round;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-image: url(foo.png) 60;
        border-image-source: var(--test);
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: url("foo.png") 60;
        border-image-source: var(--test);
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        -webkit-border-image: url("test.png") 60;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-image: url("test.png") 60;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        -webkit-border-image: url("test.png") 60;
        border-image: url("test.png") 60;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-image: url("test.png") 60;
        border-image: url("test.png") 60;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        -webkit-border-image: url("test.png") 60;
        border-image-source: url(foo.png);
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-image: url("test.png") 60;
        border-image-source: url("foo.png");
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border: 1px solid red;
        border-image: url(test.png) 60;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid red;
        border-image: url("test.png") 60;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-image: url(test.png) 60;
        border: 1px solid red;
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid red;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border: 1px solid red;
        border-image: var(--border-image);
      }
    "#,
      indoc! {r#"
      .foo {
        border: 1px solid red;
        border-image: var(--border-image);
      }
    "#
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: url("test.png") 60;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-image: url("test.png") 60;
        -moz-border-image: url("test.png") 60;
        -o-border-image: url("test.png") 60;
        border-image: url("test.png") 60;
      }
    "#
      },
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        opera: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: url(foo.png) 10 40 fill / 10px round;
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: url("foo.png") 10 40 fill / 10px round;
      }
    "#
      },
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        opera: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: var(--test) 60;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-image: var(--test) 60;
        -moz-border-image: var(--test) 60;
        -o-border-image: var(--test) 60;
        border-image: var(--test) 60;
      }
    "#
      },
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        opera: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-border-image: url(foo.png) 60;
        -moz-border-image: url(foo.png) 60;
        -o-border-image: url(foo.png) 60;
        border-image: url(foo.png) 60;
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: url("foo.png") 60;
      }
    "#
      },
      Browsers {
        chrome: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 60;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-image: -webkit-gradient(linear, 0 0, 0 100%, from(#ff0f0e), to(#7773ff)) 60;
        -webkit-border-image: -webkit-linear-gradient(#ff0f0e, #7773ff) 60;
        border-image: linear-gradient(#ff0f0e, #7773ff) 60;
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 60;
      }
    "#
      },
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 60;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-image: -webkit-gradient(linear, 0 0, 0 100%, from(#ff0f0e), to(#7773ff)) 60;
        -webkit-border-image: -webkit-linear-gradient(#ff0f0e, #7773ff) 60;
        -moz-border-image: -moz-linear-gradient(#ff0f0e, #7773ff) 60;
        border-image: linear-gradient(#ff0f0e, #7773ff) 60;
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 60;
      }
    "#
      },
      Browsers {
        chrome: Some(8 << 16),
        firefox: Some(4 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 60;
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: -webkit-linear-gradient(#ff0f0e, #7773ff) 60;
        border-image: -moz-linear-gradient(#ff0f0e, #7773ff) 60;
        border-image: linear-gradient(#ff0f0e, #7773ff) 60;
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 60;
      }
    "#
      },
      Browsers {
        chrome: Some(15 << 16),
        firefox: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image-source: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
      }
    "#,
      indoc! {r#"
      .foo {
        border-image-source: -webkit-linear-gradient(#ff0f0e, #7773ff);
        border-image-source: linear-gradient(#ff0f0e, #7773ff);
        border-image-source: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
      }
    "#
      },
      Browsers {
        chrome: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) var(--foo);
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: linear-gradient(#ff0f0e, #7773ff) var(--foo);
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          border-image: linear-gradient(lab(56.208% 94.4644 98.8928), lab(51% 70.4544 -115.586)) var(--foo);
        }
      }
    "#
      },
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image-source: linear-gradient(red, green);
        border-image-source: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
      }
    "#,
      indoc! {r#"
      .foo {
        border-image-source: linear-gradient(red, green);
        border-image-source: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
      }
    "#
      },
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image-source: linear-gradient(red, green);
        border-image-source: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
      }
    "#,
      indoc! {r#"
      .foo {
        border-image-source: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
      }
    "#
      },
      Browsers {
        chrome: Some(112 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: linear-gradient(red, green);
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: linear-gradient(red, green);
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
      }
    "#
      },
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: var(--fallback);
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: var(--fallback);
        border-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
      }
    "#
      },
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: url("fallback.png") 10 40 fill / 10px;
        border-image: url("main.png") 10 40 fill / 10px space;
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: url("fallback.png") 10 40 fill / 10px;
        border-image: url("main.png") 10 40 fill / 10px space;
      }
    "#
      },
      Browsers {
        chrome: Some(50 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-image: url("fallback.png") 10 40 fill / 10px;
        border-image: url("main.png") 10 40 fill / 10px space;
      }
    "#,
      indoc! {r#"
      .foo {
        border-image: url("main.png") 10 40 fill / 10px space;
      }
    "#
      },
      Browsers {
        chrome: Some(56 << 16),
        ..Browsers::default()
      },
    );

    minify_test(".foo { border: none green }", ".foo{border:green}");
  }

  #[test]
  pub fn test_border_radius() {
    test(
      r#"
      .foo {
        border-radius: 10px 100px 10px 100px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-radius: 10px 100px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-radius: 10px 100px 10px 100px / 120px 120px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-radius: 10px 100px / 120px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-top-left-radius: 10px 120px;
        border-top-right-radius: 100px 120px;
        border-bottom-right-radius: 100px 120px;
        border-bottom-left-radius: 10px 120px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-radius: 10px 100px 100px 10px / 120px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-top-left-radius: 4px 2px;
        border-top-right-radius: 3px 4px;
        border-bottom-right-radius: 6px 2px;
        border-bottom-left-radius: 3px 4px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-radius: 4px 3px 6px / 2px 4px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-top-left-radius: 1% 2%;
        border-top-right-radius: 3% 4%;
        border-bottom-right-radius: 5% 6%;
        border-bottom-left-radius: 7% 8%;
      }
    "#,
      indoc! {r#"
      .foo {
        border-radius: 1% 3% 5% 7% / 2% 4% 6% 8%;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-radius: 10px 100px 10px 100px / 120px 120px;
        border-start-start-radius: 10px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-radius: 10px 100px / 120px;
        border-start-start-radius: 10px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-start-start-radius: 10px;
        border-radius: 10px 100px 10px 100px / 120px 120px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-radius: 10px 100px / 120px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-top-left-radius: 10px 120px;
        border-top-right-radius: 100px 120px;
        border-start-start-radius: 10px;
        border-bottom-right-radius: 100px 120px;
        border-bottom-left-radius: 10px 120px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-top-left-radius: 10px 120px;
        border-top-right-radius: 100px 120px;
        border-start-start-radius: 10px;
        border-bottom-right-radius: 100px 120px;
        border-bottom-left-radius: 10px 120px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-radius: 10px;
        border-top-left-radius: 20px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-radius: 20px 10px 10px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        border-radius: 10px;
        border-top-left-radius: var(--test);
      }
    "#,
      indoc! {r#"
      .foo {
        border-radius: 10px;
        border-top-left-radius: var(--test);
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        -webkit-border-radius: 10px 100px 10px 100px;
        -moz-border-radius: 10px 100px 10px 100px;
        border-radius: 10px 100px 10px 100px;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-radius: 10px 100px;
        -moz-border-radius: 10px 100px;
        border-radius: 10px 100px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        -webkit-border-radius: 10px 100px 10px 100px;
        -moz-border-radius: 20px;
        border-radius: 30px;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-radius: 10px 100px;
        -moz-border-radius: 20px;
        border-radius: 30px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        -webkit-border-top-left-radius: 10px;
        -moz-border-top-left-radius: 10px;
        border-top-left-radius: 10px;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-top-left-radius: 10px;
        -moz-border-top-left-radius: 10px;
        border-top-left-radius: 10px;
      }
    "#
      },
    );

    prefix_test(
      r#"
      .foo {
        border-radius: 30px;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-radius: 30px;
        -moz-border-radius: 30px;
        border-radius: 30px;
      }
    "#
      },
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(3 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-top-left-radius: 30px;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-top-left-radius: 30px;
        -moz-border-top-left-radius: 30px;
        border-top-left-radius: 30px;
      }
    "#
      },
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(3 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-border-radius: 30px;
        -moz-border-radius: 30px;
        border-radius: 30px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-radius: 30px;
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        firefox: Some(46 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-border-top-left-radius: 30px;
        -moz-border-top-left-radius: 30px;
        border-top-left-radius: 30px;
      }
    "#,
      indoc! {r#"
      .foo {
        border-top-left-radius: 30px;
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        firefox: Some(46 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-border-radius: 30px;
        -moz-border-radius: 30px;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-radius: 30px;
        -moz-border-radius: 30px;
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        firefox: Some(46 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-border-top-left-radius: 30px;
        -moz-border-top-right-radius: 30px;
        border-bottom-right-radius: 30px;
        border-bottom-left-radius: 30px;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-top-left-radius: 30px;
        -moz-border-top-right-radius: 30px;
        border-bottom-right-radius: 30px;
        border-bottom-left-radius: 30px;
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        firefox: Some(46 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-radius: var(--test);
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-border-radius: var(--test);
        -moz-border-radius: var(--test);
        border-radius: var(--test);
      }
    "#
      },
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(3 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-start-start-radius: 5px;
      }
    "#,
      indoc! {r#"
      .foo:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        border-top-left-radius: 5px;
      }

      .foo:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        border-top-right-radius: 5px;
      }
    "#
      },
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-start-start-radius: 5px;
        border-start-end-radius: 10px;
      }
    "#,
      indoc! {r#"
      .foo:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        border-top-left-radius: 5px;
        border-top-right-radius: 10px;
      }

      .foo:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        border-top-left-radius: 10px;
        border-top-right-radius: 5px;
      }
    "#
      },
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-end-end-radius: 10px;
        border-end-start-radius: 5px;
      }
    "#,
      indoc! {r#"
      .foo:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        border-bottom-right-radius: 10px;
        border-bottom-left-radius: 5px;
      }

      .foo:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        border-bottom-right-radius: 5px;
        border-bottom-left-radius: 10px;
      }
    "#
      },
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-start-start-radius: var(--radius);
      }
    "#,
      indoc! {r#"
      .foo:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        border-top-left-radius: var(--radius);
      }

      .foo:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        border-top-right-radius: var(--radius);
      }
    "#
      },
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        border-start-start-radius: var(--start);
        border-start-end-radius: var(--end);
      }
    "#,
      indoc! {r#"
      .foo:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        border-top-left-radius: var(--start);
        border-top-right-radius: var(--end);
      }

      .foo:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        border-top-right-radius: var(--start);
        border-top-left-radius: var(--end);
      }
    "#
      },
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  pub fn test_outline() {
    test(
      r#"
      .foo {
        outline-width: 2px;
        outline-style: solid;
        outline-color: blue;
      }
    "#,
      indoc! {r#"
      .foo {
        outline: 2px solid #00f;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        outline: 2px solid blue;
      }
    "#,
      indoc! {r#"
      .foo {
        outline: 2px solid #00f;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        outline: 2px solid red;
        outline-color: blue;
      }
    "#,
      indoc! {r#"
      .foo {
        outline: 2px solid #00f;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        outline: 2px solid yellow;
        outline-color: var(--color);
      }
    "#,
      indoc! {r#"
      .foo {
        outline: 2px solid #ff0;
        outline-color: var(--color);
      }
    "#
      },
    );

    prefix_test(
      ".foo { outline-color: lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          outline-color: #b32323;
          outline-color: lab(40% 56.6 39);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { outline: 2px solid lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          outline: 2px solid #b32323;
          outline: 2px solid lab(40% 56.6 39);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { outline: var(--width) solid lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          outline: var(--width) solid #b32323;
        }

        @supports (color: lab(0% 0 0)) {
          .foo {
            outline: var(--width) solid lab(40% 56.6 39);
          }
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  pub fn test_margin() {
    test(
      r#"
      .foo {
        margin-left: 10px;
        margin-right: 10px;
        margin-top: 20px;
        margin-bottom: 20px;
      }
    "#,
      indoc! {r#"
      .foo {
        margin: 20px 10px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        margin-block-start: 15px;
        margin-block-end: 15px;
      }
    "#,
      indoc! {r#"
      .foo {
        margin-block: 15px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        margin-left: 10px;
        margin-right: 10px;
        margin-inline-start: 15px;
        margin-inline-end: 15px;
        margin-top: 20px;
        margin-bottom: 20px;

      }
    "#,
      indoc! {r#"
      .foo {
        margin-left: 10px;
        margin-right: 10px;
        margin-inline: 15px;
        margin-top: 20px;
        margin-bottom: 20px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        margin: 10px;
        margin-top: 20px;
      }
    "#,
      indoc! {r#"
      .foo {
        margin: 20px 10px 10px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        margin: 10px;
        margin-top: var(--top);
      }
    "#,
      indoc! {r#"
      .foo {
        margin: 10px;
        margin-top: var(--top);
      }
    "#
      },
    );

    prefix_test(
      r#"
      .foo {
        margin-inline-start: 2px;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        margin-left: 2px;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        margin-left: 2px;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        margin-right: 2px;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        margin-right: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        margin-inline-start: 2px;
        margin-inline-end: 4px;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        margin-left: 2px;
        margin-right: 4px;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        margin-left: 2px;
        margin-right: 4px;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        margin-left: 4px;
        margin-right: 2px;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        margin-left: 4px;
        margin-right: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        margin-inline: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        margin-left: 2px;
        margin-right: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        margin-block-start: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        margin-top: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        margin-block-end: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        margin-bottom: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        margin-inline-start: 2px;
        margin-inline-end: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        margin-inline-start: 2px;
        margin-inline-end: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        margin-inline: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        margin-inline-start: 2px;
        margin-inline-end: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        margin-inline-start: 2px;
        margin-inline-end: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        margin-inline: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        margin-inline: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        margin-inline: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_length() {
    for prop in &[
      "margin-right",
      "margin",
      "padding-right",
      "padding",
      "width",
      "height",
      "min-height",
      "max-height",
      "line-height",
      "border-radius",
    ] {
      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: 22px;
          {}: max(4%, 22px);
        }}
      "#,
          prop, prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: 22px;
          {}: max(4%, 22px);
        }}
      "#
          },
          prop, prop
        ),
        Browsers {
          safari: Some(10 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: 22px;
          {}: max(4%, 22px);
        }}
      "#,
          prop, prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: max(4%, 22px);
        }}
      "#
          },
          prop
        ),
        Browsers {
          safari: Some(14 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: 22px;
          {}: max(2cqw, 22px);
        }}
      "#,
          prop, prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: 22px;
          {}: max(2cqw, 22px);
        }}
      "#
          },
          prop, prop
        ),
        Browsers {
          safari: Some(14 << 16),
          ..Browsers::default()
        },
      );
      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: 22px;
          {}: max(2cqw, 22px);
        }}
      "#,
          prop, prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: max(2cqw, 22px);
        }}
      "#
          },
          prop
        ),
        Browsers {
          safari: Some(16 << 16),
          ..Browsers::default()
        },
      );
    }
  }

  #[test]
  pub fn test_padding() {
    test(
      r#"
      .foo {
        padding-left: 10px;
        padding-right: 10px;
        padding-top: 20px;
        padding-bottom: 20px;
      }
    "#,
      indoc! {r#"
      .foo {
        padding: 20px 10px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        padding-block-start: 15px;
        padding-block-end: 15px;
      }
    "#,
      indoc! {r#"
      .foo {
        padding-block: 15px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        padding-left: 10px;
        padding-right: 10px;
        padding-inline-start: 15px;
        padding-inline-end: 15px;
        padding-top: 20px;
        padding-bottom: 20px;

      }
    "#,
      indoc! {r#"
      .foo {
        padding-left: 10px;
        padding-right: 10px;
        padding-inline: 15px;
        padding-top: 20px;
        padding-bottom: 20px;
      }
    "#
      },
    );

    prefix_test(
      r#"
      .foo {
        padding-inline-start: 2px;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        padding-left: 2px;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        padding-left: 2px;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        padding-right: 2px;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        padding-right: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        padding-inline-start: 2px;
        padding-inline-end: 4px;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        padding-left: 2px;
        padding-right: 4px;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        padding-left: 2px;
        padding-right: 4px;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        padding-left: 4px;
        padding-right: 2px;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        padding-left: 4px;
        padding-right: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        padding-inline-start: var(--padding);
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        padding-left: var(--padding);
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        padding-left: var(--padding);
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        padding-right: var(--padding);
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        padding-right: var(--padding);
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        padding-inline: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        padding-left: 2px;
        padding-right: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        padding-block-start: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        padding-top: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        padding-block-end: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        padding-bottom: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        padding-top: 1px;
        padding-left: 2px;
        padding-bottom: 3px;
        padding-right: 4px;
      }
    "#,
      indoc! {r#"
      .foo {
        padding: 1px 4px 3px 2px;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        padding-inline-start: 2px;
        padding-inline-end: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        padding-inline-start: 2px;
        padding-inline-end: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        padding-inline-start: 2px;
        padding-inline-end: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        padding-inline: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_scroll_padding() {
    prefix_test(
      r#"
      .foo {
        scroll-padding-inline: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        scroll-padding-inline: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_size() {
    prefix_test(
      r#"
      .foo {
        block-size: 25px;
        inline-size: 25px;
        min-block-size: 25px;
        min-inline-size: 25px;
      }
    "#,
      indoc! {r#"
      .foo {
        height: 25px;
        min-height: 25px;
        width: 25px;
        min-width: 25px;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        block-size: 25px;
        min-block-size: 25px;
        inline-size: 25px;
        min-inline-size: 25px;
      }
    "#,
      indoc! {r#"
      .foo {
        block-size: 25px;
        min-block-size: 25px;
        inline-size: 25px;
        min-inline-size: 25px;
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        block-size: var(--size);
        min-block-size: var(--size);
        inline-size: var(--size);
        min-inline-size: var(--size);
      }
    "#,
      indoc! {r#"
      .foo {
        height: var(--size);
        min-height: var(--size);
        width: var(--size);
        min-width: var(--size);
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    for (in_prop, out_prop) in [
      ("width", "width"),
      ("height", "height"),
      ("block-size", "height"),
      ("inline-size", "width"),
      ("min-width", "min-width"),
      ("min-height", "min-height"),
      ("min-block-size", "min-height"),
      ("min-inline-size", "min-width"),
      ("max-width", "max-width"),
      ("max-height", "max-height"),
      ("max-block-size", "max-height"),
      ("max-inline-size", "max-width"),
    ] {
      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: stretch;
        }}
      "#,
          in_prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: -webkit-fill-available;
          {}: -moz-available;
          {}: stretch;
        }}
      "#},
          out_prop, out_prop, out_prop
        ),
        Browsers {
          safari: Some(8 << 16),
          firefox: Some(4 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: -webkit-fill-available;
        }}
      "#,
          in_prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: -webkit-fill-available;
        }}
      "#},
          out_prop
        ),
        Browsers {
          safari: Some(8 << 16),
          firefox: Some(4 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: 100vw;
          {}: -webkit-fill-available;
        }}
      "#,
          in_prop, in_prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: 100vw;
          {}: -webkit-fill-available;
        }}
      "#},
          out_prop, out_prop
        ),
        Browsers {
          safari: Some(8 << 16),
          firefox: Some(4 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: fit-content;
        }}
      "#,
          in_prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: -webkit-fit-content;
          {}: -moz-fit-content;
          {}: fit-content;
        }}
      "#},
          out_prop, out_prop, out_prop
        ),
        Browsers {
          safari: Some(8 << 16),
          firefox: Some(4 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: fit-content(50%);
        }}
      "#,
          in_prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: fit-content(50%);
        }}
      "#},
          out_prop
        ),
        Browsers {
          safari: Some(8 << 16),
          firefox: Some(4 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: min-content;
        }}
      "#,
          in_prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: -webkit-min-content;
          {}: -moz-min-content;
          {}: min-content;
        }}
      "#},
          out_prop, out_prop, out_prop
        ),
        Browsers {
          safari: Some(8 << 16),
          firefox: Some(4 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: max-content;
        }}
      "#,
          in_prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: -webkit-max-content;
          {}: -moz-max-content;
          {}: max-content;
        }}
      "#},
          out_prop, out_prop, out_prop
        ),
        Browsers {
          safari: Some(8 << 16),
          firefox: Some(4 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: 100%;
          {}: max-content;
        }}
      "#,
          in_prop, in_prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: 100%;
          {}: max-content;
        }}
      "#},
          out_prop, out_prop
        ),
        Browsers {
          safari: Some(8 << 16),
          firefox: Some(4 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: var(--fallback);
          {}: max-content;
        }}
      "#,
          in_prop, in_prop
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: var(--fallback);
          {}: max-content;
        }}
      "#},
          out_prop, out_prop
        ),
        Browsers {
          safari: Some(8 << 16),
          firefox: Some(4 << 16),
          ..Browsers::default()
        },
      );
    }

    minify_test(".foo { aspect-ratio: auto }", ".foo{aspect-ratio:auto}");
    minify_test(".foo { aspect-ratio: 2 / 3 }", ".foo{aspect-ratio:2/3}");
    minify_test(".foo { aspect-ratio: auto 2 / 3 }", ".foo{aspect-ratio:auto 2/3}");
    minify_test(".foo { aspect-ratio: 2 / 3 auto }", ".foo{aspect-ratio:auto 2/3}");

    minify_test(
      ".foo { width: 200px; width: var(--foo); }",
      ".foo{width:200px;width:var(--foo)}",
    );
    minify_test(
      ".foo { width: var(--foo); width: 200px; }",
      ".foo{width:var(--foo);width:200px}",
    );
  }

  #[test]
  pub fn test_background() {
    test(
      r#"
      .foo {
        background: url(img.png);
        background-position-x: 20px;
        background-position-y: 10px;
        background-size: 50px 100px;
        background-repeat: repeat no-repeat;
      }
    "#,
      indoc! {r#"
      .foo {
        background: url("img.png") 20px 10px / 50px 100px repeat-x;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        background-color: red;
        background-position: 0% 0%;
        background-size: auto;
        background-repeat: repeat;
        background-clip: border-box;
        background-origin: padding-box;
        background-attachment: scroll;
        background-image: none
      }
    "#,
      indoc! {r#"
      .foo {
        background: red;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        background-color: gray;
        background-position: 40% 50%;
        background-size: 10em auto;
        background-repeat: round;
        background-clip: border-box;
        background-origin: border-box;
        background-attachment: fixed;
        background-image: url('chess.png');
      }
    "#,
      indoc! {r#"
      .foo {
        background: gray url("chess.png") 40% / 10em round fixed border-box;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        background: url(img.png), url(test.jpg) gray;
        background-position-x: right 20px, 10px;
        background-position-y: top 20px, 15px;
        background-size: 50px 50px, auto;
        background-repeat: repeat no-repeat, no-repeat;
      }
    "#,
      indoc! {r#"
      .foo {
        background: url("img.png") right 20px top 20px / 50px 50px repeat-x, gray url("test.jpg") 10px 15px no-repeat;
      }
    "#
      },
    );

    minify_test(
      r#"
      .foo {
        background-position: center center;
      }
    "#,
      indoc! {".foo{background-position:50%}"
      },
    );

    test(
      r#"
      .foo {
        background: url(img.png) gray;
        background-clip: content-box;
        -webkit-background-clip: text;
      }
    "#,
      indoc! {r#"
      .foo {
        background: gray url("img.png") padding-box content-box;
        -webkit-background-clip: text;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        background: url(img.png) gray;
        -webkit-background-clip: text;
        background-clip: content-box;
      }
    "#,
      indoc! {r#"
      .foo {
        background: gray url("img.png");
        -webkit-background-clip: text;
        background-clip: content-box;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        background: url(img.png) gray;
        background-position: var(--pos);
      }
    "#,
      indoc! {r#"
      .foo {
        background: gray url("img.png");
        background-position: var(--pos);
      }
    "#
      },
    );

    minify_test(
      ".foo { background-position: bottom left }",
      ".foo{background-position:0 100%}",
    );
    minify_test(
      ".foo { background-position: left 10px center }",
      ".foo{background-position:10px 50%}",
    );
    minify_test(
      ".foo { background-position: right 10px center }",
      ".foo{background-position:right 10px center}",
    );
    minify_test(
      ".foo { background-position: right 10px top 20px }",
      ".foo{background-position:right 10px top 20px}",
    );
    minify_test(
      ".foo { background-position: left 10px top 20px }",
      ".foo{background-position:10px 20px}",
    );
    minify_test(
      ".foo { background-position: left 10px bottom 20px }",
      ".foo{background-position:left 10px bottom 20px}",
    );
    minify_test(
      ".foo { background-position: left 10px top }",
      ".foo{background-position:10px 0}",
    );
    minify_test(
      ".foo { background-position: bottom right }",
      ".foo{background-position:100% 100%}",
    );

    minify_test(
      ".foo { background: url('img-sprite.png') no-repeat bottom right }",
      ".foo{background:url(img-sprite.png) 100% 100% no-repeat}",
    );
    minify_test(".foo { background: transparent }", ".foo{background:0 0}");

    minify_test(".foo { background: url(\"data:image/svg+xml,%3Csvg width='168' height='24' xmlns='http://www.w3.org/2000/svg'%3E%3C/svg%3E\") }", ".foo{background:url(\"data:image/svg+xml,%3Csvg width='168' height='24' xmlns='http://www.w3.org/2000/svg'%3E%3C/svg%3E\")}");

    test(
      r#"
      .foo {
        background: url(img.png);
        background-clip: text;
      }
    "#,
      indoc! {r#"
      .foo {
        background: url("img.png") text;
      }
    "#
      },
    );

    prefix_test(
      r#"
      .foo {
        background: url(img.png);
        background-clip: text;
      }
    "#,
      indoc! {r#"
      .foo {
        background: url("img.png");
        -webkit-background-clip: text;
        background-clip: text;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background: url(img.png);
        background-clip: text;
      }
    "#,
      indoc! {r#"
      .foo {
        background: url("img.png") text;
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background: url(img.png) text;
      }
    "#,
      indoc! {r#"
      .foo {
        background: url("img.png");
        -webkit-background-clip: text;
        background-clip: text;
      }
    "#
      },
      Browsers {
        chrome: Some(45 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background: url(img.png);
        -webkit-background-clip: text;
      }
    "#,
      indoc! {r#"
      .foo {
        background: url("img.png");
        -webkit-background-clip: text;
      }
    "#
      },
      Browsers {
        chrome: Some(45 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background: url(img.png);
        background-clip: text;
      }
    "#,
      indoc! {r#"
      .foo {
        background: url("img.png");
        -webkit-background-clip: text;
        background-clip: text;
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background-image: url(img.png);
        background-clip: text;
      }
    "#,
      indoc! {r#"
      .foo {
        background-image: url("img.png");
        -webkit-background-clip: text;
        background-clip: text;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-background-clip: text;
        background-clip: text;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-background-clip: text;
        background-clip: text;
      }
    "#
      },
      Browsers {
        chrome: Some(45 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background-image: url(img.png);
        background-clip: text;
      }
    "#,
      indoc! {r#"
      .foo {
        background-image: url("img.png");
        background-clip: text;
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    minify_test(".foo { background: none center }", ".foo{background:50%}");
    minify_test(".foo { background: none }", ".foo{background:0 0}");

    prefix_test(
      r#"
      .foo {
        background: lab(51.5117% 43.3777 -29.0443);
      }
    "#,
      indoc! {r#"
      .foo {
        background: #af5cae;
        background: lab(51.5117% 43.3777 -29.0443);
      }
    "#
      },
      Browsers {
        chrome: Some(95 << 16),
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background: lab(51.5117% 43.3777 -29.0443) url(foo.png);
      }
    "#,
      indoc! {r#"
      .foo {
        background: #af5cae url("foo.png");
        background: lab(51.5117% 43.3777 -29.0443) url("foo.png");
      }
    "#
      },
      Browsers {
        chrome: Some(95 << 16),
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background: lab(51.5117% 43.3777 -29.0443) linear-gradient(lab(52.2319% 40.1449 59.9171), lab(47.7776% -34.2947 -7.65904));
      }
    "#,
      indoc! {r#"
      .foo {
        background: #af5cae linear-gradient(#c65d07, #00807c);
        background: lab(51.5117% 43.3777 -29.0443) linear-gradient(lab(52.2319% 40.1449 59.9171), lab(47.7776% -34.2947 -7.65904));
      }
    "#
      },
      Browsers {
        chrome: Some(95 << 16),
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    test(
      ".foo { background: calc(var(--v) / 0.3)",
      indoc! {r#"
      .foo {
        background: calc(var(--v) / .3);
      }
    "#},
    );

    prefix_test(
      r#"
      .foo {
        background-color: #4263eb;
        background-color: color(display-p3 0 .5 1);
      }
    "#,
      indoc! {r#"
      .foo {
        background-color: #4263eb;
        background-color: color(display-p3 0 .5 1);
      }
    "#
      },
      Browsers {
        chrome: Some(99 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-color: #4263eb;
        background-color: color(display-p3 0 .5 1);
      }
    "#,
      indoc! {r#"
      .foo {
        background-color: color(display-p3 0 .5 1);
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(red, green);
        background-image: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#,
      indoc! {r#"
      .foo {
        background-image: linear-gradient(red, green);
        background-image: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#
      },
      Browsers {
        chrome: Some(99 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(red, green);
        background-image: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#,
      indoc! {r#"
      .foo {
        background-image: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: #4263eb;
        background: color(display-p3 0 .5 1);
      }
    "#,
      indoc! {r#"
      .foo {
        background: #4263eb;
        background: color(display-p3 0 .5 1);
      }
    "#
      },
      Browsers {
        chrome: Some(99 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: #4263eb;
        background: color(display-p3 0 .5 1);
      }
    "#,
      indoc! {r#"
      .foo {
        background: color(display-p3 0 .5 1);
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: linear-gradient(red, green);
        background: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#,
      indoc! {r#"
      .foo {
        background: linear-gradient(red, green);
        background: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#
      },
      Browsers {
        chrome: Some(99 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: red;
        background: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#,
      indoc! {r#"
      .foo {
        background: red;
        background: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#
      },
      Browsers {
        chrome: Some(99 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: linear-gradient(red, green);
        background: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#,
      indoc! {r#"
      .foo {
        background: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: var(--fallback);
        background: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#,
      indoc! {r#"
      .foo {
        background: var(--fallback);
        background: linear-gradient(lch(50% 132 50), lch(50% 130 150));
      }
    "#
      },
      Browsers {
        chrome: Some(99 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: red url(foo.png);
        background: lch(50% 132 50) url(foo.png);
      }
    "#,
      indoc! {r#"
      .foo {
        background: red url("foo.png");
        background: lch(50% 132 50) url("foo.png");
      }
    "#
      },
      Browsers {
        chrome: Some(99 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  pub fn test_flex() {
    test(
      r#"
      .foo {
        flex-direction: column;
        flex-wrap: wrap;
      }
    "#,
      indoc! {r#"
      .foo {
        flex-flow: column wrap;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-direction: row;
        flex-wrap: wrap;
      }
    "#,
      indoc! {r#"
      .foo {
        flex-flow: wrap;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-direction: row;
        flex-wrap: nowrap;
      }
    "#,
      indoc! {r#"
      .foo {
        flex-flow: row;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-direction: column;
        flex-wrap: nowrap;
      }
    "#,
      indoc! {r#"
      .foo {
        flex-flow: column;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 1;
        flex-shrink: 1;
        flex-basis: 0%;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 1;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 1;
        flex-shrink: 1;
        flex-basis: 0;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 1 1 0;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 1;
        flex-shrink: 1;
        flex-basis: 0px;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 1 1 0;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 1;
        flex-shrink: 2;
        flex-basis: 0%;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 1 2;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 2;
        flex-shrink: 1;
        flex-basis: 0%;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 2;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 2;
        flex-shrink: 2;
        flex-basis: 0%;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 2 2;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 1;
        flex-shrink: 1;
        flex-basis: 10px;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 10px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 2;
        flex-shrink: 1;
        flex-basis: 10px;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 2 10px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 1;
        flex-shrink: 0;
        flex-basis: 0%;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 1 0;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 1;
        flex-shrink: 0;
        flex-basis: auto;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 1 0 auto;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex-grow: 1;
        flex-shrink: 1;
        flex-basis: auto;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: auto;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex: 0 0;
        flex-grow: 1;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 1 0;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        flex: 0 0;
        flex-grow: var(--grow);
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 0 0;
        flex-grow: var(--grow);
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        align-content: center;
        justify-content: center;
      }
    "#,
      indoc! {r#"
      .foo {
        place-content: center;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        align-content: first baseline;
        justify-content: safe right;
      }
    "#,
      indoc! {r#"
      .foo {
        place-content: baseline safe right;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        place-content: first baseline unsafe left;
      }
    "#,
      indoc! {r#"
      .foo {
        place-content: baseline unsafe left;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        place-content: center center;
      }
    "#,
      indoc! {r#"
      .foo {
        place-content: center;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        align-self: center;
        justify-self: center;
      }
    "#,
      indoc! {r#"
      .foo {
        place-self: center;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        align-self: center;
        justify-self: unsafe left;
      }
    "#,
      indoc! {r#"
      .foo {
        place-self: center unsafe left;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        align-items: center;
        justify-items: center;
      }
    "#,
      indoc! {r#"
      .foo {
        place-items: center;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        align-items: center;
        justify-items: legacy left;
      }
    "#,
      indoc! {r#"
      .foo {
        place-items: center legacy left;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        place-items: center;
        justify-items: var(--justify);
      }
    "#,
      indoc! {r#"
      .foo {
        place-items: center;
        justify-items: var(--justify);
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        row-gap: 10px;
        column-gap: 20px;
      }
    "#,
      indoc! {r#"
      .foo {
        gap: 10px 20px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        row-gap: 10px;
        column-gap: 10px;
      }
    "#,
      indoc! {r#"
      .foo {
        gap: 10px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        gap: 10px;
        column-gap: 20px;
      }
    "#,
      indoc! {r#"
      .foo {
        gap: 10px 20px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        column-gap: 20px;
        gap: 10px;
      }
    "#,
      indoc! {r#"
      .foo {
        gap: 10px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        row-gap: normal;
        column-gap: 20px;
      }
    "#,
      indoc! {r#"
      .foo {
        gap: normal 20px;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        -webkit-flex-grow: 1;
        -webkit-flex-shrink: 1;
        -webkit-flex-basis: auto;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-flex: auto;
      }
    "#
      },
    );
    test(
      r#"
      .foo {
        -webkit-flex-grow: 1;
        -webkit-flex-shrink: 1;
        -webkit-flex-basis: auto;
        flex-grow: 1;
        flex-shrink: 1;
        flex-basis: auto;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-flex: auto;
        flex: auto;
      }
    "#
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-box-orient: horizontal;
        -webkit-box-direction: normal;
        flex-direction: row;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-orient: horizontal;
        -webkit-box-direction: normal;
        -webkit-flex-direction: row;
        flex-direction: row;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        flex-direction: row;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-orient: horizontal;
        -moz-box-orient: horizontal;
        -webkit-box-direction: normal;
        -moz-box-direction: normal;
        -webkit-flex-direction: row;
        -ms-flex-direction: row;
        flex-direction: row;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-box-orient: horizontal;
        -webkit-box-direction: normal;
        -moz-box-orient: horizontal;
        -moz-box-direction: normal;
        -webkit-flex-direction: row;
        -ms-flex-direction: row;
        flex-direction: row;
      }
    "#,
      indoc! {r#"
      .foo {
        flex-direction: row;
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        flex-wrap: wrap;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-lines: multiple;
        -moz-box-lines: multiple;
        -webkit-flex-wrap: wrap;
        -ms-flex-wrap: wrap;
        flex-wrap: wrap;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-box-lines: multiple;
        -moz-box-lines: multiple;
        -webkit-flex-wrap: wrap;
        -ms-flex-wrap: wrap;
        flex-wrap: wrap;
      }
    "#,
      indoc! {r#"
      .foo {
        flex-wrap: wrap;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        flex-flow: row wrap;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-orient: horizontal;
        -moz-box-orient: horizontal;
        -webkit-box-direction: normal;
        -moz-box-direction: normal;
        -webkit-flex-flow: wrap;
        -ms-flex-flow: wrap;
        flex-flow: wrap;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-box-orient: horizontal;
        -moz-box-orient: horizontal;
        -webkit-box-direction: normal;
        -moz-box-direction: normal;
        -webkit-flex-flow: wrap;
        -ms-flex-flow: wrap;
        flex-flow: wrap;
      }
    "#,
      indoc! {r#"
      .foo {
        flex-flow: wrap;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        flex-grow: 1;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-flex: 1;
        -moz-box-flex: 1;
        -ms-flex-positive: 1;
        -webkit-flex-grow: 1;
        flex-grow: 1;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-box-flex: 1;
        -moz-box-flex: 1;
        -ms-flex-positive: 1;
        -webkit-flex-grow: 1;
        flex-grow: 1;
      }
    "#,
      indoc! {r#"
      .foo {
        flex-grow: 1;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        flex-shrink: 1;
      }
    "#,
      indoc! {r#"
      .foo {
        -ms-flex-negative: 1;
        -webkit-flex-shrink: 1;
        flex-shrink: 1;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -ms-flex-negative: 1;
        -webkit-flex-shrink: 1;
        flex-shrink: 1;
      }
    "#,
      indoc! {r#"
      .foo {
        flex-shrink: 1;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        flex-basis: 1px;
      }
    "#,
      indoc! {r#"
      .foo {
        -ms-flex-preferred-size: 1px;
        -webkit-flex-basis: 1px;
        flex-basis: 1px;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -ms-flex-preferred-size: 1px;
        -webkit-flex-basis: 1px;
        flex-basis: 1px;
      }
    "#,
      indoc! {r#"
      .foo {
        flex-basis: 1px;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        flex: 1;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-flex: 1;
        -moz-box-flex: 1;
        -webkit-flex: 1;
        -ms-flex: 1;
        flex: 1;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-box-flex: 1;
        -moz-box-flex: 1;
        -webkit-flex: 1;
        -ms-flex: 1;
        flex: 1;
      }
    "#,
      indoc! {r#"
      .foo {
        flex: 1;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        align-content: space-between;
      }
    "#,
      indoc! {r#"
      .foo {
        -ms-flex-line-pack: justify;
        -webkit-align-content: space-between;
        align-content: space-between;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -ms-flex-line-pack: justify;
        -webkit-align-content: space-between;
        align-content: space-between;
      }
    "#,
      indoc! {r#"
      .foo {
        align-content: space-between;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        justify-content: space-between;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-pack: justify;
        -moz-box-pack: justify;
        -ms-flex-pack: justify;
        -webkit-justify-content: space-between;
        justify-content: space-between;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-box-pack: justify;
        -moz-box-pack: justify;
        -ms-flex-pack: justify;
        -webkit-justify-content: space-between;
        justify-content: space-between;
      }
    "#,
      indoc! {r#"
      .foo {
        justify-content: space-between;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        place-content: space-between flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        -ms-flex-line-pack: justify;
        -webkit-box-pack: end;
        -moz-box-pack: end;
        -ms-flex-pack: end;
        -webkit-align-content: space-between;
        align-content: space-between;
        -webkit-justify-content: flex-end;
        justify-content: flex-end;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -ms-flex-line-pack: justify;
        -webkit-box-pack: end;
        -moz-box-pack: end;
        -ms-flex-pack: end;
        -webkit-align-content: space-between;
        -webkit-justify-content: flex-end;
        place-content: space-between flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        place-content: space-between flex-end;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        place-content: space-between flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        align-content: space-between;
        justify-content: flex-end;
      }
    "#},
      Browsers {
        chrome: Some(30 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        place-content: space-between flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        place-content: space-between flex-end;
      }
    "#},
      Browsers {
        chrome: Some(60 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        align-self: flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        -ms-flex-item-align: end;
        -webkit-align-self: flex-end;
        align-self: flex-end;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -ms-flex-item-align: end;
        -webkit-align-self: flex-end;
        align-self: flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        align-self: flex-end;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        place-self: center flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        -ms-flex-item-align: center;
        -webkit-align-self: center;
        align-self: center;
        justify-self: flex-end;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -ms-flex-item-align: center;
        -webkit-align-self: center;
        place-self: center flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        place-self: center flex-end;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        place-self: center flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        align-self: center;
        justify-self: flex-end;
      }
    "#},
      Browsers {
        chrome: Some(57 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        place-self: center flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        place-self: center flex-end;
      }
    "#},
      Browsers {
        chrome: Some(59 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        align-items: flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-align: end;
        -moz-box-align: end;
        -ms-flex-align: end;
        -webkit-align-items: flex-end;
        align-items: flex-end;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-box-align: end;
        -moz-box-align: end;
        -ms-flex-align: end;
        -webkit-align-items: flex-end;
        align-items: flex-end;
      }
    "#,
      indoc! {r#"
      .foo {
        align-items: flex-end;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        place-items: flex-end center;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-align: end;
        -moz-box-align: end;
        -ms-flex-align: end;
        -webkit-align-items: flex-end;
        align-items: flex-end;
        justify-items: center;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-box-align: end;
        -moz-box-align: end;
        -ms-flex-align: end;
        -webkit-align-items: flex-end;
        place-items: flex-end center;
      }
    "#,
      indoc! {r#"
      .foo {
        place-items: flex-end center;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        place-items: flex-end center;
      }
    "#,
      indoc! {r#"
      .foo {
        align-items: flex-end;
        justify-items: center;
      }
    "#},
      Browsers {
        safari: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        order: 1;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-ordinal-group: 1;
        -moz-box-ordinal-group: 1;
        -ms-flex-order: 1;
        -webkit-order: 1;
        order: 1;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(4 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-box-ordinal-group: 1;
        -moz-box-ordinal-group: 1;
        -ms-flex-order: 1;
        -webkit-order: 1;
        order: 1;
      }
    "#,
      indoc! {r#"
      .foo {
        order: 1;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -ms-flex: 0 0 8%;
        flex: 0 0 5%;
      }
    "#,
      indoc! {r#"
      .foo {
        -ms-flex: 0 0 8%;
        flex: 0 0 5%;
      }
    "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_font() {
    test(
      r#"
      .foo {
        font-family: "Helvetica", "Times New Roman", sans-serif;
        font-size: 12px;
        font-weight: bold;
        font-style: italic;
        font-stretch: expanded;
        font-variant-caps: small-caps;
        line-height: 1.2em;
      }
    "#,
      indoc! {r#"
      .foo {
        font: italic small-caps bold expanded 12px / 1.2em Helvetica, Times New Roman, sans-serif;
      }
    "#
      },
    );

    minify_test(
      r#"
      .foo {
        font-family: "Helvetica", "Times New Roman", sans-serif;
        font-size: 12px;
        font-weight: bold;
        font-style: italic;
        font-stretch: expanded;
        font-variant-caps: small-caps;
        line-height: 1.2em;
      }
    "#,
      indoc! {".foo{font:italic small-caps 700 125% 12px/1.2em Helvetica,Times New Roman,sans-serif}"
      },
    );

    test(
      r#"
      .foo {
        font: 12px "Helvetica", "Times New Roman", sans-serif;
        line-height: 1.2em;
      }
    "#,
      indoc! {r#"
      .foo {
        font: 12px / 1.2em Helvetica, Times New Roman, sans-serif;
      }
    "#
      },
    );

    test(
      r#"
      .foo {
        font: 12px "Helvetica", "Times New Roman", sans-serif;
        line-height: var(--lh);
      }
    "#,
      indoc! {r#"
      .foo {
        font: 12px Helvetica, Times New Roman, sans-serif;
        line-height: var(--lh);
      }
    "#
      },
    );

    minify_test(
      r#"
      .foo {
        font-family: "Helvetica", "Times New Roman", sans-serif;
        font-size: 12px;
        font-stretch: expanded;
      }
    "#,
      indoc! {".foo{font-family:Helvetica,Times New Roman,sans-serif;font-size:12px;font-stretch:125%}"
      },
    );

    test(
      r#"
      .foo {
        font-family: "Helvetica", "Times New Roman", sans-serif;
        font-size: 12px;
        font-weight: bold;
        font-style: italic;
        font-stretch: expanded;
        font-variant-caps: all-small-caps;
        line-height: 1.2em;
      }
    "#,
      indoc! {r#"
      .foo {
        font: italic bold expanded 12px / 1.2em Helvetica, Times New Roman, sans-serif;
        font-variant-caps: all-small-caps;
      }
    "#
      },
    );

    minify_test(
      ".foo { font: normal normal 600 9px/normal Charcoal; }",
      ".foo{font:600 9px Charcoal}",
    );
    minify_test(
      ".foo { font: normal normal 500 medium/normal Charcoal; }",
      ".foo{font:500 medium Charcoal}",
    );
    minify_test(
      ".foo { font: normal normal 400 medium Charcoal; }",
      ".foo{font:400 medium Charcoal}",
    );
    minify_test(
      ".foo { font: normal normal 500 medium/10px Charcoal; }",
      ".foo{font:500 medium/10px Charcoal}",
    );
    minify_test(
      ".foo { font-family: 'sans-serif'; }",
      ".foo{font-family:\"sans-serif\"}",
    );
    minify_test(".foo { font-family: sans-serif; }", ".foo{font-family:sans-serif}");
    minify_test(".foo { font-family: 'default'; }", ".foo{font-family:\"default\"}");
    minify_test(".foo { font-family: default; }", ".foo{font-family:default}");
    minify_test(".foo { font-family: 'inherit'; }", ".foo{font-family:\"inherit\"}");
    minify_test(".foo { font-family: inherit; }", ".foo{font-family:inherit}");
    minify_test(".foo { font-family: inherit test; }", ".foo{font-family:inherit test}");
    minify_test(
      ".foo { font-family: 'inherit test'; }",
      ".foo{font-family:inherit test}",
    );
    minify_test(".foo { font-family: revert; }", ".foo{font-family:revert}");
    minify_test(".foo { font-family: 'revert'; }", ".foo{font-family:\"revert\"}");
    minify_test(".foo { font-family: revert-layer; }", ".foo{font-family:revert-layer}");
    minify_test(
      ".foo { font-family: revert-layer, serif; }",
      ".foo{font-family:revert-layer,serif}",
    );
    minify_test(
      ".foo { font-family: 'revert', sans-serif; }",
      ".foo{font-family:\"revert\",sans-serif}",
    );
    minify_test(
      ".foo { font-family: 'revert', foo, sans-serif; }",
      ".foo{font-family:\"revert\",foo,sans-serif}",
    );
    minify_test(".foo { font-family: ''; }", ".foo{font-family:\"\"}");

    // font-family in @font-face
    minify_test(
      "@font-face { font-family: 'revert'; }",
      "@font-face{font-family:\"revert\"}",
    );
    minify_test(
      "@font-face { font-family: 'revert-layer'; }",
      "@font-face{font-family:\"revert-layer\"}",
    );

    prefix_test(
      r#"
      .foo {
        font-family: Helvetica, system-ui, sans-serif;
      }
    "#,
      indoc! {r#"
      .foo {
        font-family: Helvetica, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, Noto Sans, Ubuntu, Cantarell, Helvetica Neue, sans-serif;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        font: 100%/1.5 Helvetica, system-ui, sans-serif;
      }
    "#,
      indoc! {r#"
      .foo {
        font: 100% / 1.5 Helvetica, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, Noto Sans, Ubuntu, Cantarell, Helvetica Neue, sans-serif;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Noto Sans", sans-serif, "Apple Color Emoji", "Segoe UI Emoji", "Segoe UI Symbol", "Noto Color Emoji";
      }
    "#,
      indoc! {r#"
      .foo {
        font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, Noto Sans, Ubuntu, Cantarell, Helvetica Neue, Arial, sans-serif, Apple Color Emoji, Segoe UI Emoji, Segoe UI Symbol, Noto Color Emoji;
      }
    "#
      },
      Browsers {
        firefox: Some(91 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        font-size: 22px;
        font-size: max(2cqw, 22px);
      }
    "#,
      indoc! {r#"
      .foo {
        font-size: 22px;
        font-size: max(2cqw, 22px);
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        font-size: 22px;
        font-size: max(2cqw, 22px);
      }
    "#,
      indoc! {r#"
      .foo {
        font-size: max(2cqw, 22px);
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        font-size: 22px;
        font-size: xxx-large;
      }
    "#,
      indoc! {r#"
      .foo {
        font-size: 22px;
        font-size: xxx-large;
      }
    "#
      },
      Browsers {
        chrome: Some(70 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        font-size: 22px;
        font-size: xxx-large;
      }
    "#,
      indoc! {r#"
      .foo {
        font-size: xxx-large;
      }
    "#
      },
      Browsers {
        chrome: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        font-weight: 700;
        font-weight: 789;
      }
    "#,
      indoc! {r#"
      .foo {
        font-weight: 700;
        font-weight: 789;
      }
    "#
      },
      Browsers {
        chrome: Some(60 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        font-weight: 700;
        font-weight: 789;
      }
    "#,
      indoc! {r#"
      .foo {
        font-weight: 789;
      }
    "#
      },
      Browsers {
        chrome: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        font-family: Helvetica;
        font-family: system-ui;
      }
    "#,
      indoc! {r#"
      .foo {
        font-family: Helvetica;
        font-family: system-ui;
      }
    "#
      },
      Browsers {
        chrome: Some(50 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        font-family: Helvetica;
        font-family: system-ui;
      }
    "#,
      indoc! {r#"
      .foo {
        font-family: system-ui;
      }
    "#
      },
      Browsers {
        chrome: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        font-style: oblique;
        font-style: oblique 40deg;
      }
    "#,
      indoc! {r#"
      .foo {
        font-style: oblique;
        font-style: oblique 40deg;
      }
    "#
      },
      Browsers {
        firefox: Some(50 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        font-style: oblique;
        font-style: oblique 40deg;
      }
    "#,
      indoc! {r#"
      .foo {
        font-style: oblique 40deg;
      }
    "#
      },
      Browsers {
        firefox: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        font: 22px Helvetica;
        font: xxx-large system-ui;
      }
    "#,
      indoc! {r#"
      .foo {
        font: 22px Helvetica;
        font: xxx-large system-ui;
      }
    "#
      },
      Browsers {
        chrome: Some(70 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        font: 22px Helvetica;
        font: xxx-large system-ui;
      }
    "#,
      indoc! {r#"
      .foo {
        font: xxx-large system-ui;
      }
    "#
      },
      Browsers {
        chrome: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        font: var(--fallback);
        font: xxx-large system-ui;
      }
    "#,
      indoc! {r#"
      .foo {
        font: var(--fallback);
        font: xxx-large system-ui;
      }
    "#
      },
      Browsers {
        chrome: Some(50 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_vertical_align() {
    minify_test(".foo { vertical-align: middle }", ".foo{vertical-align:middle}");
    minify_test(".foo { vertical-align: 0.3em }", ".foo{vertical-align:.3em}");
  }

  #[test]
  fn test_selectors() {
    minify_test(":nth-col(2n) {width: 20px}", ":nth-col(2n){width:20px}");
    minify_test(":nth-col(10n-1) {width: 20px}", ":nth-col(10n-1){width:20px}");
    minify_test(":nth-col(-n+2) {width: 20px}", ":nth-col(-n+2){width:20px}");
    minify_test(":nth-col(even) {width: 20px}", ":nth-col(2n){width:20px}");
    minify_test(":nth-col(odd) {width: 20px}", ":nth-col(odd){width:20px}");
    minify_test(":nth-last-col(2n) {width: 20px}", ":nth-last-col(2n){width:20px}");
    minify_test(":nth-last-col(10n-1) {width: 20px}", ":nth-last-col(10n-1){width:20px}");
    minify_test(":nth-last-col(-n+2) {width: 20px}", ":nth-last-col(-n+2){width:20px}");
    minify_test(":nth-last-col(even) {width: 20px}", ":nth-last-col(2n){width:20px}");
    minify_test(":nth-last-col(odd) {width: 20px}", ":nth-last-col(odd){width:20px}");
    minify_test(":nth-child(odd) {width: 20px}", ":nth-child(odd){width:20px}");
    minify_test(":nth-child(2n) {width: 20px}", ":nth-child(2n){width:20px}");
    minify_test(":nth-child(2n+1) {width: 20px}", ":nth-child(odd){width:20px}");
    minify_test(":first-child {width: 20px}", ":first-child{width:20px}");
    minify_test(":nth-child(1) {width: 20px}", ":first-child{width:20px}");
    minify_test(":nth-last-child(1) {width: 20px}", ":last-child{width:20px}");
    minify_test(":nth-of-type(1) {width: 20px}", ":first-of-type{width:20px}");
    minify_test(":nth-last-of-type(1) {width: 20px}", ":last-of-type{width:20px}");
    minify_test(
      ":nth-child(even of li.important) {width: 20px}",
      ":nth-child(2n of li.important){width:20px}",
    );
    minify_test(
      ":nth-child(1 of li.important) {width: 20px}",
      ":nth-child(1 of li.important){width:20px}",
    );
    minify_test(
      ":nth-last-child(even of li.important) {width: 20px}",
      ":nth-last-child(2n of li.important){width:20px}",
    );
    minify_test(
      ":nth-last-child(1 of li.important) {width: 20px}",
      ":nth-last-child(1 of li.important){width:20px}",
    );
    minify_test(
      ":nth-last-child(1 of.important) {width: 20px}",
      ":nth-last-child(1 of .important){width:20px}",
    );

    minify_test("[foo=\"baz\"] {color:red}", "[foo=baz]{color:red}");
    minify_test("[foo=\"foo bar\"] {color:red}", "[foo=foo\\ bar]{color:red}");
    minify_test("[foo=\"foo bar baz\"] {color:red}", "[foo=\"foo bar baz\"]{color:red}");
    minify_test("[foo=\"\"] {color:red}", "[foo=\"\"]{color:red}");
    minify_test(
      ".test:not([foo=\"bar\"]) {color:red}",
      ".test:not([foo=bar]){color:red}",
    );
    minify_test(".test + .foo {color:red}", ".test+.foo{color:red}");
    minify_test(".test ~ .foo {color:red}", ".test~.foo{color:red}");
    minify_test(".test .foo {color:red}", ".test .foo{color:red}");
    minify_test(
      ".custom-range::-webkit-slider-thumb:active {color:red}",
      ".custom-range::-webkit-slider-thumb:active{color:red}",
    );
    minify_test(".test:not(.foo, .bar) {color:red}", ".test:not(.foo,.bar){color:red}");
    minify_test(".test:is(.foo, .bar) {color:red}", ".test:is(.foo,.bar){color:red}");
    minify_test(
      ".test:where(.foo, .bar) {color:red}",
      ".test:where(.foo,.bar){color:red}",
    );
    minify_test(
      ".test:where(.foo, .bar) {color:red}",
      ".test:where(.foo,.bar){color:red}",
    );
    minify_test(":host {color:red}", ":host{color:red}");
    minify_test(":host(.foo) {color:red}", ":host(.foo){color:red}");
    minify_test("::slotted(span) {color:red", "::slotted(span){color:red}");
    minify_test(
      "custom-element::part(foo) {color:red}",
      "custom-element::part(foo){color:red}",
    );
    minify_test(".sm\\:text-5xl { font-size: 3rem }", ".sm\\:text-5xl{font-size:3rem}");
    minify_test("a:has(> img) {color:red}", "a:has(>img){color:red}");
    minify_test("dt:has(+ dt) {color:red}", "dt:has(+dt){color:red}");
    minify_test(
      "section:not(:has(h1, h2, h3, h4, h5, h6)) {color:red}",
      "section:not(:has(h1,h2,h3,h4,h5,h6)){color:red}",
    );
    minify_test(
      ":has(.sibling ~ .target) {color:red}",
      ":has(.sibling~.target){color:red}",
    );
    minify_test(".x:has(> .a > .b) {color:red}", ".x:has(>.a>.b){color:red}");
    minify_test(".x:has(.bar, #foo) {color:red}", ".x:has(.bar,#foo){color:red}");
    minify_test(".x:has(span + span) {color:red}", ".x:has(span+span){color:red}");
    minify_test("a:has(:visited) {color:red}", "a:has(:visited){color:red}");
    for element in [
      "-webkit-scrollbar",
      "-webkit-scrollbar-button",
      "-webkit-scrollbar-track",
      "-webkit-scrollbar-track-piece",
      "-webkit-scrollbar-thumb",
      "-webkit-scrollbar-corner",
      "-webkit-resizer",
    ] {
      for class in [
        "enabled",
        "disabled",
        "hover",
        "active",
        "horizontal",
        "vertical",
        "decrement",
        "increment",
        "start",
        "end",
        "double-button",
        "single-button",
        "no-button",
        "corner-present",
        "window-inactive",
      ] {
        minify_test(
          &format!("::{}:{} {{color:red}}", element, class),
          &format!("::{}:{}{{color:red}}", element, class),
        );
      }
    }
    for class in [
      "horizontal",
      "vertical",
      "decrement",
      "increment",
      "start",
      "end",
      "double-button",
      "single-button",
      "no-button",
      "corner-present",
      "window-inactive",
    ] {
      error_test(
        &format!(":{} {{color:red}}", class),
        ParserError::SelectorError(SelectorError::InvalidPseudoClassBeforeWebKitScrollbar),
      );
    }
    for element in [
      "-webkit-scrollbar",
      "-webkit-scrollbar-button",
      "-webkit-scrollbar-track",
      "-webkit-scrollbar-track-piece",
      "-webkit-scrollbar-thumb",
      "-webkit-scrollbar-corner",
      "-webkit-resizer",
    ] {
      error_test(
        &format!("::{}:focus {{color:red}}", element),
        ParserError::SelectorError(SelectorError::InvalidPseudoClassAfterWebKitScrollbar),
      );
    }

    error_test(
      "a::first-letter:last-child {color:red}",
      ParserError::SelectorError(SelectorError::InvalidPseudoClassAfterPseudoElement),
    );
    minify_test(
      "a:last-child::first-letter {color:red}",
      "a:last-child:first-letter{color:red}",
    );

    prefix_test(
      ".test:not(.foo, .bar) {color:red}",
      indoc! {r#"
      .test:not(:-webkit-any(.foo, .bar)) {
        color: red;
      }

      .test:not(:is(.foo, .bar)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".test:not(.foo, .bar) {color:red}",
      indoc! {r#"
      .test:not(.foo, .bar) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );

    minify_test("a:lang(en) {color:red}", "a:lang(en){color:red}");
    minify_test("a:lang(en, fr) {color:red}", "a:lang(en,fr){color:red}");
    minify_test("a:lang('en') {color:red}", "a:lang(en){color:red}");
    minify_test(
      "a:-webkit-any(.foo, .bar) {color:red}",
      "a:-webkit-any(.foo,.bar){color:red}",
    );
    minify_test("a:-moz-any(.foo, .bar) {color:red}", "a:-moz-any(.foo,.bar){color:red}");

    prefix_test(
      "a:is(.foo, .bar) {color:red}",
      indoc! {r#"
      a:-webkit-any(.foo, .bar) {
        color: red;
      }

      a:-moz-any(.foo, .bar) {
        color: red;
      }

      a:is(.foo, .bar) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(11 << 16),
        firefox: Some(50 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:is(.foo > .bar) {color:red}",
      indoc! {r#"
      a:is(.foo > .bar) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(11 << 16),
        firefox: Some(50 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:lang(en, fr) {color:red}",
      indoc! {r#"
      a:-webkit-any(:lang(en), :lang(fr)) {
        color: red;
      }

      a:-moz-any(:lang(en), :lang(fr)) {
        color: red;
      }

      a:is(:lang(en), :lang(fr)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(11 << 16),
        firefox: Some(50 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:lang(en, fr) {color:red}",
      indoc! {r#"
      a:is(:lang(en), :lang(fr)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        firefox: Some(88 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:lang(en, fr) {color:red}",
      indoc! {r#"
      a:lang(en, fr) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:dir(rtl) {color:red}",
      indoc! {r#"
      a:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        color: red;
      }

      a:-moz-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        color: red;
      }

      a:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(11 << 16),
        firefox: Some(50 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:dir(ltr) {color:red}",
      indoc! {r#"
      a:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        color: red;
      }

      a:not(:-moz-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        color: red;
      }

      a:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(11 << 16),
        firefox: Some(50 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:dir(rtl) {color:red}",
      indoc! {r#"
      a:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        firefox: Some(88 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:dir(ltr) {color:red}",
      indoc! {r#"
      a:not(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        firefox: Some(88 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:dir(rtl) {color:red}",
      indoc! {r#"
      a:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:dir(ltr) {color:red}",
      indoc! {r#"
      a:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:is(:dir(rtl)) {color:red}",
      indoc! {r#"
      a:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:where(:dir(rtl)) {color:red}",
      indoc! {r#"
      a:where(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:has(:dir(rtl)) {color:red}",
      indoc! {r#"
      a:has(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:not(:dir(rtl)) {color:red}",
      indoc! {r#"
      a:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:dir(rtl)::after {color:red}",
      indoc! {r#"
      a:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi):after {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      "a:dir(rtl) div {color:red}",
      indoc! {r#"
      a:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) div {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    minify_test(".foo::cue {color: red}", ".foo::cue{color:red}");
    minify_test(".foo::cue-region {color: red}", ".foo::cue-region{color:red}");
    minify_test(".foo::cue(b) {color: red}", ".foo::cue(b){color:red}");
    minify_test(".foo::cue-region(b) {color: red}", ".foo::cue-region(b){color:red}");
    minify_test(
      "::cue(v[voice='active']) {color: yellow;}",
      "::cue(v[voice=active]){color:#ff0}",
    );
    minify_test(":foo(bar) { color: yellow }", ":foo(bar){color:#ff0}");
    minify_test("::foo(bar) { color: yellow }", "::foo(bar){color:#ff0}");
    minify_test("::foo(*) { color: yellow }", "::foo(*){color:#ff0}");

    minify_test(":is(.foo) { color: yellow }", ".foo{color:#ff0}");
    minify_test(":is(#foo) { color: yellow }", "#foo{color:#ff0}");
    minify_test("a:is(.foo) { color: yellow }", "a.foo{color:#ff0}");
    minify_test("a:is([data-test]) { color: yellow }", "a[data-test]{color:#ff0}");
    minify_test(".foo:is(a) { color: yellow }", ".foo:is(a){color:#ff0}");
    minify_test(".foo:is(*|a) { color: yellow }", ".foo:is(*|a){color:#ff0}");
    minify_test(".foo:is(*) { color: yellow }", ".foo:is(*){color:#ff0}");
    minify_test(
      "@namespace svg url(http://www.w3.org/2000/svg); .foo:is(svg|a) { color: yellow }",
      "@namespace svg \"http://www.w3.org/2000/svg\";.foo:is(svg|a){color:#ff0}",
    );
    minify_test("a:is(.foo .bar) { color: yellow }", "a:is(.foo .bar){color:#ff0}");
    minify_test(":is(.foo, .bar) { color: yellow }", ":is(.foo,.bar){color:#ff0}");
    minify_test("a:is(:not(.foo)) { color: yellow }", "a:not(.foo){color:#ff0}");
    minify_test("a:is(:first-child) { color: yellow }", "a:first-child{color:#ff0}");
    minify_test("a:is(:has(.foo)) { color: yellow }", "a:has(.foo){color:#ff0}");
    minify_test("a:is(:is(.foo)) { color: yellow }", "a.foo{color:#ff0}");
    minify_test(":host(:hover) {color: red}", ":host(:hover){color:red}");
    minify_test("::slotted(:hover) {color: red}", "::slotted(:hover){color:red}");

    minify_test(
      ":root::view-transition {position: fixed}",
      ":root::view-transition{position:fixed}",
    );
    minify_test(
      ":root:active-view-transition {position: fixed}",
      ":root:active-view-transition{position:fixed}",
    );
    minify_test(
      ":root:active-view-transition-type(slide-in) {position: fixed}",
      ":root:active-view-transition-type(slide-in){position:fixed}",
    );
    minify_test(
      ":root:active-view-transition-type(slide-in, reverse) {position: fixed}",
      ":root:active-view-transition-type(slide-in,reverse){position:fixed}",
    );

    for name in &[
      "view-transition-group",
      "view-transition-image-pair",
      "view-transition-new",
      "view-transition-old",
    ] {
      minify_test(
        &format!(":root::{}(*) {{position: fixed}}", name),
        &format!(":root::{}(*){{position:fixed}}", name),
      );
      minify_test(
        &format!(":root::{}(*.class) {{position: fixed}}", name),
        &format!(":root::{}(*.class){{position:fixed}}", name),
      );
      minify_test(
        &format!(":root::{}(*.class.class) {{position: fixed}}", name),
        &format!(":root::{}(*.class.class){{position:fixed}}", name),
      );
      minify_test(
        &format!(":root::{}(foo) {{position: fixed}}", name),
        &format!(":root::{}(foo){{position:fixed}}", name),
      );
      minify_test(
        &format!(":root::{}(foo.class) {{position: fixed}}", name),
        &format!(":root::{}(foo.class){{position:fixed}}", name),
      );
      minify_test(
        &format!(":root::{}(foo.bar.baz) {{position: fixed}}", name),
        &format!(":root::{}(foo.bar.baz){{position:fixed}}", name),
      );
      minify_test(
        &format!(":root::{}(foo):only-child {{position: fixed}}", name),
        &format!(":root::{}(foo):only-child{{position:fixed}}", name),
      );
      minify_test(
        &format!(":root::{}(foo.bar.baz):only-child {{position: fixed}}", name),
        &format!(":root::{}(foo.bar.baz):only-child{{position:fixed}}", name),
      );
      minify_test(
        &format!(":root::{}(.foo) {{position: fixed}}", name),
        &format!(":root::{}(.foo){{position:fixed}}", name),
      );
      minify_test(
        &format!(":root::{}(.foo.bar) {{position: fixed}}", name),
        &format!(":root::{}(.foo.bar){{position:fixed}}", name),
      );
      error_test(
        &format!(":root::{}(foo):first-child {{position: fixed}}", name),
        ParserError::SelectorError(SelectorError::InvalidPseudoClassAfterPseudoElement),
      );
      error_test(
        &format!(":root::{}(foo)::before {{position: fixed}}", name),
        ParserError::SelectorError(SelectorError::InvalidState),
      );
      error_test(
        &format!(":root::{}(*.*) {{position: fixed}}", name),
        ParserError::SelectorError(SelectorError::InvalidState),
      );
      error_test(
        &format!(":root::{}(*. cls) {{position: fixed}}", name),
        ParserError::SelectorError(SelectorError::InvalidState),
      );
      error_test(
        &format!(":root::{}(foo .bar) {{position: fixed}}", name),
        ParserError::SelectorError(SelectorError::InvalidState),
      );
      error_test(
        &format!(":root::{}(*.cls. c) {{position: fixed}}", name),
        ParserError::SelectorError(SelectorError::InvalidState),
      );
      error_test(
        &format!(":root::{}(*.cls>cls) {{position: fixed}}", name),
        ParserError::SelectorError(SelectorError::InvalidState),
      );
      error_test(
        &format!(":root::{}(*.cls.foo.*) {{position: fixed}}", name),
        ParserError::SelectorError(SelectorError::InvalidState),
      );
    }

    minify_test(".foo ::deep .bar {width: 20px}", ".foo ::deep .bar{width:20px}");
    minify_test(".foo::deep .bar {width: 20px}", ".foo::deep .bar{width:20px}");
    minify_test(".foo ::deep.bar {width: 20px}", ".foo ::deep.bar{width:20px}");
    minify_test(".foo ::unknown .bar {width: 20px}", ".foo ::unknown .bar{width:20px}");
    minify_test(
      ".foo ::unknown(foo) .bar {width: 20px}",
      ".foo ::unknown(foo) .bar{width:20px}",
    );
    minify_test(
      ".foo ::unknown:only-child {width: 20px}",
      ".foo ::unknown:only-child{width:20px}",
    );
    minify_test(
      ".foo ::unknown(.foo) .bar {width: 20px}",
      ".foo ::unknown(.foo) .bar{width:20px}",
    );
    minify_test(
      ".foo ::unknown(.foo .bar / .baz) .bar {width: 20px}",
      ".foo ::unknown(.foo .bar / .baz) .bar{width:20px}",
    );
    minify_test(
      ".foo ::unknown(something(foo)) .bar {width: 20px}",
      ".foo ::unknown(something(foo)) .bar{width:20px}",
    );
    minify_test(
      ".foo ::unknown([abc]) .bar {width: 20px}",
      ".foo ::unknown([abc]) .bar{width:20px}",
    );

    let deep_options = ParserOptions {
      flags: ParserFlags::DEEP_SELECTOR_COMBINATOR,
      ..ParserOptions::default()
    };

    error_test(
      ".foo >>> .bar {width: 20px}",
      ParserError::SelectorError(SelectorError::DanglingCombinator),
    );
    error_test(
      ".foo /deep/ .bar {width: 20px}",
      ParserError::SelectorError(SelectorError::DanglingCombinator),
    );
    minify_test_with_options(
      ".foo >>> .bar {width: 20px}",
      ".foo>>>.bar{width:20px}",
      deep_options.clone(),
    );
    minify_test_with_options(
      ".foo /deep/ .bar {width: 20px}",
      ".foo /deep/ .bar{width:20px}",
      deep_options.clone(),
    );

    let pure_css_module_options = ParserOptions {
      css_modules: Some(crate::css_modules::Config {
        pure: true,
        ..Default::default()
      }),
      ..ParserOptions::default()
    };

    minify_error_test_with_options(
      "div {width: 20px}",
      MinifyErrorKind::ImpureCSSModuleSelector,
      pure_css_module_options.clone(),
    );
    minify_error_test_with_options(
      ":global(.foo) {width: 20px}",
      MinifyErrorKind::ImpureCSSModuleSelector,
      pure_css_module_options.clone(),
    );
    minify_error_test_with_options(
      "[foo=bar] {width: 20px}",
      MinifyErrorKind::ImpureCSSModuleSelector,
      pure_css_module_options.clone(),
    );
    minify_error_test_with_options(
      "div, .foo {width: 20px}",
      MinifyErrorKind::ImpureCSSModuleSelector,
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      ":local(.foo) {width: 20px}",
      "._8Z4fiW_foo{width:20px}",
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      "div.my-class {color: red;}",
      "div._8Z4fiW_my-class{color:red}",
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      "#id {color: red;}",
      "#_8Z4fiW_id{color:red}",
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      "a .my-class{color: red;}",
      "a ._8Z4fiW_my-class{color:red}",
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      ".my-class a {color: red;}",
      "._8Z4fiW_my-class a{color:red}",
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      ".my-class:is(a) {color: red;}",
      "._8Z4fiW_my-class:is(a){color:red}",
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      "div:has(.my-class) {color: red;}",
      "div:has(._8Z4fiW_my-class){color:red}",
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      ".foo { html &:hover { a_value: some-value; } }",
      "._8Z4fiW_foo{html &:hover{a_value:some-value}}",
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      ".foo { span { color: red; } }",
      "._8Z4fiW_foo{& span{color:red}}",
      pure_css_module_options.clone(),
    );
    minify_error_test_with_options(
      "html { .foo { span { color: red; } } }",
      MinifyErrorKind::ImpureCSSModuleSelector,
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      ".foo { div { span { color: red; } } }",
      "._8Z4fiW_foo{& div{& span{color:red}}}",
      pure_css_module_options.clone(),
    );
    minify_error_test_with_options(
      "@scope (div) { .foo { color: red } }",
      MinifyErrorKind::ImpureCSSModuleSelector,
      pure_css_module_options.clone(),
    );
    minify_error_test_with_options(
      "@scope (.a) to (div) { .foo { color: red } }",
      MinifyErrorKind::ImpureCSSModuleSelector,
      pure_css_module_options.clone(),
    );
    minify_error_test_with_options(
      "@scope (.a) to (.b) { div { color: red } }",
      MinifyErrorKind::ImpureCSSModuleSelector,
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      "@scope (.a) to (.b) { .foo { color: red } }",
      "@scope(._8Z4fiW_a) to (._8Z4fiW_b){._8Z4fiW_foo{color:red}}",
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      "/* cssmodules-pure-no-check */ :global(.foo) { color: red }",
      ".foo{color:red}",
      pure_css_module_options.clone(),
    );
    minify_test_with_options(
      "/*! some license */ /* cssmodules-pure-no-check */ :global(.foo) { color: red }",
      "/*! some license */\n.foo{color:red}",
      pure_css_module_options.clone(),
    );

    error_test(
      "input.defaultCheckbox::before h1 {width: 20px}",
      ParserError::SelectorError(SelectorError::UnexpectedSelectorAfterPseudoElement(Token::Ident(
        "h1".into(),
      ))),
    );
    error_test(
      "input.defaultCheckbox::before .my-class {width: 20px}",
      ParserError::SelectorError(SelectorError::UnexpectedSelectorAfterPseudoElement(Token::Delim('.'))),
    );
    error_test(
      "input.defaultCheckbox::before.my-class {width: 20px}",
      ParserError::SelectorError(SelectorError::UnexpectedSelectorAfterPseudoElement(Token::Delim('.'))),
    );
    error_test(
      "input.defaultCheckbox::before #id {width: 20px}",
      ParserError::SelectorError(SelectorError::UnexpectedSelectorAfterPseudoElement(Token::IDHash(
        "id".into(),
      ))),
    );
    error_test(
      "input.defaultCheckbox::before#id {width: 20px}",
      ParserError::SelectorError(SelectorError::UnexpectedSelectorAfterPseudoElement(Token::IDHash(
        "id".into(),
      ))),
    );
    error_test(
      "input.defaultCheckbox::before [attr] {width: 20px}",
      ParserError::SelectorError(SelectorError::UnexpectedSelectorAfterPseudoElement(
        Token::SquareBracketBlock,
      )),
    );
    error_test(
      "input.defaultCheckbox::before[attr] {width: 20px}",
      ParserError::SelectorError(SelectorError::UnexpectedSelectorAfterPseudoElement(
        Token::SquareBracketBlock,
      )),
    );
  }

  #[test]
  fn test_keyframes() {
    minify_test(
      r#"
      @keyframes "test" {
        100% {
          background: blue
        }
      }
    "#,
      "@keyframes test{to{background:#00f}}",
    );
    minify_test(
      r#"
      @keyframes test {
        100% {
          background: blue
        }
      }
    "#,
      "@keyframes test{to{background:#00f}}",
    );

    // named animation range percentages
    minify_test(
      r#"
      @keyframes test {
        entry 0% {
          background: blue
        }
        exit 100% {
          background: green
        }
      }
    "#,
      "@keyframes test{entry 0%{background:#00f}exit 100%{background:green}}",
    );

    // CSS-wide keywords and `none` cannot remove quotes.
    minify_test(
      r#"
      @keyframes "revert" {
        from {
          background: green;
        }
      }
    "#,
      "@keyframes \"revert\"{0%{background:green}}",
    );

    minify_test(
      r#"
      @keyframes "none" {
        from {
          background: green;
        }
      }
    "#,
      "@keyframes \"none\"{0%{background:green}}",
    );

    // named animation ranges cannot be used with to or from
    minify_test(
      r#"
      @keyframes test {
        entry to {
          background: blue
        }
      }
    "#,
      "@keyframes test{}",
    );

    // CSS-wide keywords without quotes throws an error.
    error_test(
      r#"
      @keyframes revert {}
    "#,
      ParserError::UnexpectedToken(Token::Ident("revert".into())),
    );

    error_test(
      r#"
      @keyframes revert-layer {}
    "#,
      ParserError::UnexpectedToken(Token::Ident("revert-layer".into())),
    );

    error_test(
      r#"
      @keyframes none {}
    "#,
      ParserError::UnexpectedToken(Token::Ident("none".into())),
    );

    error_test(
      r#"
      @keyframes NONE {}
    "#,
      ParserError::UnexpectedToken(Token::Ident("NONE".into())),
    );

    minify_test(
      r#"
      @-webkit-keyframes test {
        from {
          background: green;
          background-color: red;
        }

        100% {
          background: blue
        }
      }
    "#,
      "@-webkit-keyframes test{0%{background:red}to{background:#00f}}",
    );
    minify_test(
      r#"
      @-moz-keyframes test {
        from {
          background: green;
          background-color: red;
        }

        100% {
          background: blue
        }
      }
    "#,
      "@-moz-keyframes test{0%{background:red}to{background:#00f}}",
    );
    minify_test(r#"
      @-webkit-keyframes test {
        from {
          background: green;
          background-color: red;
        }

        100% {
          background: blue
        }
      }
      @-moz-keyframes test {
        from {
          background: green;
          background-color: red;
        }

        100% {
          background: blue
        }
      }
    "#, "@-webkit-keyframes test{0%{background:red}to{background:#00f}}@-moz-keyframes test{0%{background:red}to{background:#00f}}");

    prefix_test(
      r#"
      @keyframes test {
        from {
          background: green;
        }
        to {
          background: blue
        }
      }
    "#,
      indoc! { r#"
      @-webkit-keyframes test {
        from {
          background: green;
        }

        to {
          background: #00f;
        }
      }

      @-moz-keyframes test {
        from {
          background: green;
        }

        to {
          background: #00f;
        }
      }

      @keyframes test {
        from {
          background: green;
        }

        to {
          background: #00f;
        }
      }
    "#},
      Browsers {
        safari: Some(5 << 16),
        firefox: Some(6 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @-webkit-keyframes test {
        from {
          background: green;
        }

        to {
          background: blue;
        }
      }
      @-moz-keyframes test {
        from {
          background: green;
        }

        to {
          background: blue;
        }
      }
      @keyframes test {
        from {
          background: green;
        }
        to {
          background: blue
        }
      }
    "#,
      indoc! { r#"
      @keyframes test {
        from {
          background: green;
        }

        to {
          background: #00f;
        }
      }
    "#},
      Browsers {
        safari: Some(10 << 16),
        firefox: Some(17 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @-webkit-keyframes test1 {
        from {
          background: green;
        }

        to {
          background: blue;
        }
      }

      @-moz-keyframes test2 {
        from {
          background: green;
        }

        to {
          background: blue;
        }
      }

      @keyframes test3 {
        from {
          background: green;
        }
        to {
          background: blue
        }
      }
    "#,
      indoc! { r#"
      @-webkit-keyframes test1 {
        from {
          background: green;
        }

        to {
          background: #00f;
        }
      }

      @-moz-keyframes test2 {
        from {
          background: green;
        }

        to {
          background: #00f;
        }
      }

      @keyframes test3 {
        from {
          background: green;
        }

        to {
          background: #00f;
        }
      }
    "#},
      Browsers {
        safari: Some(10 << 16),
        firefox: Some(17 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @-webkit-keyframes test {
        from {
          background: green;
        }

        to {
          background: red;
        }
      }
      @-moz-keyframes test {
        from {
          background: green;
        }

        to {
          background: pink;
        }
      }
      @keyframes test {
        from {
          background: green;
        }
        to {
          background: blue
        }
      }
    "#,
      indoc! { r#"
      @-webkit-keyframes test {
        from {
          background: green;
        }

        to {
          background: red;
        }
      }

      @-moz-keyframes test {
        from {
          background: green;
        }

        to {
          background: pink;
        }
      }

      @keyframes test {
        from {
          background: green;
        }

        to {
          background: #00f;
        }
      }
    "#},
      Browsers {
        safari: Some(10 << 16),
        firefox: Some(17 << 16),
        ..Browsers::default()
      },
    );

    minify_test(
      r#"
      @keyframes test {
        100% {
          background: blue
        }
      }

      @keyframes test {
        100% {
          background: red
        }
      }
    "#,
      "@keyframes test{to{background:red}}",
    );
    minify_test(
      r#"
      @keyframes test {
        100% {
          background: blue
        }
      }

      @-webkit-keyframes test {
        100% {
          background: red
        }
      }
    "#,
      "@keyframes test{to{background:#00f}}@-webkit-keyframes test{to{background:red}}",
    );
  }

  #[test]
  fn test_important() {
    test(
      r#"
      .foo {
        align-items: center;
        justify-items: center !important;
      }
    "#,
      indoc! {r#"
      .foo {
        align-items: center;
        justify-items: center !important;
      }
    "#},
    );

    test(
      r#"
      .foo {
        justify-items: center !important;
        align-items: center;
      }
    "#,
      indoc! {r#"
      .foo {
        align-items: center;
        justify-items: center !important;
      }
    "#},
    );

    minify_test(
      r#"
      .foo {
        font-family: SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", "Courier New", monospace !important;
      }
    "#,
      ".foo{font-family:SFMono-Regular,Menlo,Monaco,Consolas,Liberation Mono,Courier New,monospace!important}",
    );
  }

  #[test]
  fn test_calc() {
    minify_test(".foo { width: calc(20px * 2) }", ".foo{width:40px}");
    minify_test(".foo { font-size: calc(100vw / 35) }", ".foo{font-size:2.85714vw}");
    minify_test(".foo { width: calc(20px * 2 * 3) }", ".foo{width:120px}");
    minify_test(".foo { width: calc(20px + 30px) }", ".foo{width:50px}");
    minify_test(".foo { width: calc(20px + 30px + 40px) }", ".foo{width:90px}");
    minify_test(".foo { width: calc(100% - 30px) }", ".foo{width:calc(100% - 30px)}");
    minify_test(
      ".foo { width: calc(100% - 30px + 20px) }",
      ".foo{width:calc(100% - 10px)}",
    );
    minify_test(
      ".foo { width: calc(20px + 100% - 30px) }",
      ".foo{width:calc(100% - 10px)}",
    );
    minify_test(
      ".foo { width: calc(20px + 100% + 10vw - 30px) }",
      ".foo{width:calc(100% - 10px + 10vw)}",
    );
    minify_test(
      ".foo { width: calc(20px + 100% - 30px) }",
      ".foo{width:calc(100% - 10px)}",
    );
    minify_test(
      ".foo { width: calc(2 * (100% - 20px)) }",
      ".foo{width:calc(200% - 40px)}",
    );
    minify_test(
      ".foo { width: calc((100% - 20px) * 2) }",
      ".foo{width:calc(200% - 40px)}",
    );
    minify_test(".foo { width: calc(100% - 20px * 2) }", ".foo{width:calc(100% - 40px)}");
    minify_test(".foo { width: calc(1px + 1px) }", ".foo{width:2px}");
    minify_test(".foo { width: calc(100vw / 2) }", ".foo{width:50vw}");
    minify_test(".foo { width: calc(50px - (20px - 30px)) }", ".foo{width:60px}");
    minify_test(".foo { width: calc(100px - (100px - 100%)) }", ".foo{width:100%}");
    minify_test(
      ".foo { width: calc(100px + (100px - 100%)) }",
      ".foo{width:calc(200px - 100%)}",
    );
    minify_test(
      ".foo { width: calc(1px - (2em + 3%)) }",
      ".foo{width:calc(1px + -2em - 3%)}",
    ); // TODO: fix sign
    minify_test(
      ".foo { width: calc((100vw - 50em) / 2) }",
      ".foo{width:calc(50vw - 25em)}",
    );
    minify_test(
      ".foo { width: calc(1px - (2em + 4vh + 3%)) }",
      ".foo{width:calc(1px + -2em - 4vh - 3%)}",
    ); // TODO
    minify_test(
      ".foo { width: calc(1px + (2em + (3vh + 4px))) }",
      ".foo{width:calc(2em + 3vh + 5px)}",
    );
    minify_test(
      ".foo { width: calc(1px - (2em + 4px - 6vh) / 2) }",
      ".foo{width:calc(-1em - 1px + 3vh)}",
    );
    minify_test(
      ".foo { width: calc(100% - calc(50% + 25px)) }",
      ".foo{width:calc(50% - 25px)}",
    );
    minify_test(".foo { width: calc(1px/100) }", ".foo{width:.01px}");
    minify_test(
      ".foo { width: calc(100vw / 2 - 6px + 0px) }",
      ".foo{width:calc(50vw - 6px)}",
    );
    minify_test(".foo { width: calc(1px + 1) }", ".foo{width:calc(1px + 1)}");
    minify_test(
      ".foo { width: calc( (1em - calc( 10px + 1em)) / 2) }",
      ".foo{width:-5px}",
    );
    minify_test(
      ".foo { width: calc((100px - 1em) + (-50px + 1em)) }",
      ".foo{width:50px}",
    );
    minify_test(
      ".foo { width: calc(100% + (2 * 100px) - ((75.37% - 63.5px) - 900px)) }",
      ".foo{width:calc(24.63% + 1163.5px)}",
    );
    minify_test(
      ".foo { width: calc(((((100% + (2 * 30px) + 63.5px) / 0.7537) - (100vw - 60px)) / 2) + 30px) }",
      ".foo{width:calc(66.3394% + 141.929px - 50vw)}",
    );
    minify_test(
      ".foo { width: calc(((75.37% - 63.5px) - 900px) + (2 * 100px)) }",
      ".foo{width:calc(75.37% - 763.5px)}",
    );
    minify_test(
      ".foo { width: calc((900px - (10% - 63.5px)) + (2 * 100px)) }",
      ".foo{width:calc(1163.5px - 10%)}",
    );
    minify_test(".foo { width: calc(500px/0) }", ".foo{width:calc(500px/0)}");
    minify_test(".foo { width: calc(500px/2px) }", ".foo{width:calc(500px/2px)}");
    minify_test(".foo { width: calc(100% / 3 * 3) }", ".foo{width:100%}");
    minify_test(".foo { width: calc(+100px + +100px) }", ".foo{width:200px}");
    minify_test(".foo { width: calc(+100px - +100px) }", ".foo{width:0}");
    minify_test(".foo { width: calc(200px * +1) }", ".foo{width:200px}");
    minify_test(".foo { width: calc(200px / +1) }", ".foo{width:200px}");
    minify_test(".foo { width: calc(1.1e+1px + 1.1e+1px) }", ".foo{width:22px}");
    minify_test(".foo { border-width: calc(1px + 2px) }", ".foo{border-width:3px}");
    minify_test(
      ".foo { border-width: calc(1em + 2px + 2em + 3px) }",
      ".foo{border-width:calc(3em + 5px)}",
    );

    minify_test(
      ".foo { border-width: min(1em, 2px) }",
      ".foo{border-width:min(1em,2px)}",
    );
    minify_test(
      ".foo { border-width: min(1em + 2em, 2px + 2px) }",
      ".foo{border-width:min(3em,4px)}",
    );
    minify_test(
      ".foo { border-width: min(1em + 2px, 2px + 1em) }",
      ".foo{border-width:min(1em + 2px,2px + 1em)}",
    );
    minify_test(
      ".foo { border-width: min(1em + 2px + 2px, 2px + 1em + 1px) }",
      ".foo{border-width:min(1em + 4px,3px + 1em)}",
    );
    minify_test(
      ".foo { border-width: min(2px + 1px, 3px + 4px) }",
      ".foo{border-width:3px}",
    );
    minify_test(
      ".foo { border-width: min(1px, 1em, 2px, 3in) }",
      ".foo{border-width:min(1px,1em)}",
    );

    minify_test(
      ".foo { border-width: max(1em, 2px) }",
      ".foo{border-width:max(1em,2px)}",
    );
    minify_test(
      ".foo { border-width: max(1em + 2em, 2px + 2px) }",
      ".foo{border-width:max(3em,4px)}",
    );
    minify_test(
      ".foo { border-width: max(1em + 2px, 2px + 1em) }",
      ".foo{border-width:max(1em + 2px,2px + 1em)}",
    );
    minify_test(
      ".foo { border-width: max(1em + 2px + 2px, 2px + 1em + 1px) }",
      ".foo{border-width:max(1em + 4px,3px + 1em)}",
    );
    minify_test(
      ".foo { border-width: max(2px + 1px, 3px + 4px) }",
      ".foo{border-width:7px}",
    );
    minify_test(
      ".foo { border-width: max(1px, 1em, 2px, 3in) }",
      ".foo{border-width:max(3in,1em)}",
    );

    minify_test(".foo { border-width: clamp(1px, 2px, 3px) }", ".foo{border-width:2px}");
    minify_test(".foo { border-width: clamp(1px, 10px, 3px) }", ".foo{border-width:3px}");
    minify_test(".foo { border-width: clamp(5px, 2px, 10px) }", ".foo{border-width:5px}");
    minify_test(
      ".foo { border-width: clamp(100px, 2px, 10px) }",
      ".foo{border-width:100px}",
    );
    minify_test(
      ".foo { border-width: clamp(5px + 5px, 5px + 7px, 10px + 20px) }",
      ".foo{border-width:12px}",
    );

    minify_test(
      ".foo { border-width: clamp(1em, 2px, 4vh) }",
      ".foo{border-width:clamp(1em,2px,4vh)}",
    );
    minify_test(
      ".foo { border-width: clamp(1em, 2em, 4vh) }",
      ".foo{border-width:clamp(1em,2em,4vh)}",
    );
    minify_test(
      ".foo { border-width: clamp(1em, 2vh, 4vh) }",
      ".foo{border-width:max(1em,2vh)}",
    );
    minify_test(
      ".foo { border-width: clamp(1px, 1px + 2em, 4px) }",
      ".foo{border-width:clamp(1px,1px + 2em,4px)}",
    );
    minify_test(".foo { border-width: clamp(1px, 2pt, 1in) }", ".foo{border-width:2pt}");
    minify_test(
      ".foo { width: clamp(-100px, 0px, 50% - 50vw); }",
      ".foo{width:clamp(-100px,0px,50% - 50vw)}",
    );

    minify_test(
      ".foo { top: calc(-1 * clamp(1.75rem, 8vw, 4rem)) }",
      ".foo{top:calc(-1*clamp(1.75rem,8vw,4rem))}",
    );
    minify_test(
      ".foo { top: calc(-1 * min(1.75rem, 8vw, 4rem)) }",
      ".foo{top:calc(-1*min(1.75rem,8vw))}",
    );
    minify_test(
      ".foo { top: calc(-1 * max(1.75rem, 8vw, 4rem)) }",
      ".foo{top:calc(-1*max(4rem,8vw))}",
    );
    minify_test(
      ".foo { top: calc(clamp(1.75rem, 8vw, 4rem) * -1) }",
      ".foo{top:calc(-1*clamp(1.75rem,8vw,4rem))}",
    );
    minify_test(
      ".foo { top: calc(min(1.75rem, 8vw, 4rem) * -1) }",
      ".foo{top:calc(-1*min(1.75rem,8vw))}",
    );
    minify_test(
      ".foo { top: calc(max(1.75rem, 8vw, 4rem) * -1) }",
      ".foo{top:calc(-1*max(4rem,8vw))}",
    );
    minify_test(
      ".foo { top: calc(clamp(1.75rem, 8vw, 4rem) / 2) }",
      ".foo{top:calc(clamp(1.75rem,8vw,4rem)/2)}",
    );
    minify_test(
      ".foo { top: calc(min(1.75rem, 8vw, 4rem) / 2) }",
      ".foo{top:calc(min(1.75rem,8vw)/2)}",
    );
    minify_test(
      ".foo { top: calc(max(1.75rem, 8vw, 4rem) / 2) }",
      ".foo{top:calc(max(4rem,8vw)/2)}",
    );
    minify_test(
      ".foo { top: calc(0.5 * clamp(1.75rem, 8vw, 4rem)) }",
      ".foo{top:calc(clamp(1.75rem,8vw,4rem)/2)}",
    );
    minify_test(
      ".foo { top: calc(1 * clamp(1.75rem, 8vw, 4rem)) }",
      ".foo{top:calc(clamp(1.75rem,8vw,4rem))}",
    );
    minify_test(
      ".foo { top: calc(2 * clamp(1.75rem, 8vw, 4rem) / 2) }",
      ".foo{top:calc(clamp(1.75rem,8vw,4rem))}",
    );

    minify_test(".foo { width: max(0px, 1vw) }", ".foo{width:max(0px,1vw)}");

    prefix_test(
      ".foo { border-width: clamp(1em, 2px, 4vh) }",
      indoc! { r#"
        .foo {
          border-width: max(1em, min(2px, 4vh));
        }
      "#},
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { border-width: clamp(1em, 2px, 4vh) }",
      indoc! { r#"
        .foo {
          border-width: clamp(1em, 2px, 4vh);
        }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    minify_test(".foo { width: calc(1vh + 2vh) }", ".foo{width:3vh}");
    minify_test(".foo { width: calc(1dvh + 2dvh) }", ".foo{width:3dvh}");
    minify_test(".foo { width: calc(1lvh + 2lvh) }", ".foo{width:3lvh}");
    minify_test(".foo { width: calc(1svh + 2svh) }", ".foo{width:3svh}");
    minify_test(".foo { width: calc(1sVmin + 2Svmin) }", ".foo{width:3svmin}");
    minify_test(".foo { width: calc(1ic + 2ic) }", ".foo{width:3ic}");
    minify_test(".foo { width: calc(1ric + 2ric) }", ".foo{width:3ric}");
    minify_test(".foo { width: calc(1cap + 2cap) }", ".foo{width:3cap}");
    minify_test(".foo { width: calc(1lh + 2lh) }", ".foo{width:3lh}");
    minify_test(".foo { width: calc(1x + 2x) }", ".foo{width:calc(1x + 2x)}");
    minify_test(
      ".foo { left: calc(50% - 100px + clamp(0px, calc(50vw - 50px), 100px)) }",
      ".foo{left:calc(50% - 100px + clamp(0px,50vw - 50px,100px))}",
    );
    minify_test(
      ".foo { left: calc(10px + min(10px, 1rem) + max(2px, 1vw)) }",
      ".foo{left:calc(10px + min(10px,1rem) + max(2px,1vw))}",
    );
    minify_test(".foo { width: round(22px, 5px) }", ".foo{width:20px}");
    minify_test(".foo { width: round(nearest, 22px, 5px) }", ".foo{width:20px}");
    minify_test(".foo { width: round(down, 22px, 5px) }", ".foo{width:20px}");
    minify_test(".foo { width: round(to-zero, 22px, 5px) }", ".foo{width:20px}");
    minify_test(".foo { width: round(up, 22px, 5px) }", ".foo{width:25px}");
    minify_test(".foo { width: round(23px, 5px) }", ".foo{width:25px}");
    minify_test(".foo { width: round(nearest, 23px, 5px) }", ".foo{width:25px}");
    minify_test(".foo { width: round(down, 23px, 5px) }", ".foo{width:20px}");
    minify_test(".foo { width: round(to-zero, 23px, 5px) }", ".foo{width:20px}");
    minify_test(".foo { width: round(up, 23px, 5px) }", ".foo{width:25px}");
    minify_test(".foo { width: round(22px, 5vw) }", ".foo{width:round(22px,5vw)}");
    minify_test(".foo { rotate: round(22deg, 5deg) }", ".foo{rotate:20deg}");
    minify_test(".foo { rotate: round(22deg, 5deg) }", ".foo{rotate:20deg}");
    minify_test(
      ".foo { transition-duration: round(22ms, 5ms) }",
      ".foo{transition-duration:20ms}",
    );
    minify_test(".foo { margin: round(to-zero, -23px, 5px) }", ".foo{margin:-20px}");
    minify_test(".foo { margin: round(nearest, -23px, 5px) }", ".foo{margin:-25px}");
    minify_test(".foo { margin: calc(10px * round(22, 5)) }", ".foo{margin:200px}");
    minify_test(".foo { width: rem(18px, 5px) }", ".foo{width:3px}");
    minify_test(".foo { width: rem(-18px, 5px) }", ".foo{width:-3px}");
    minify_test(".foo { width: rem(18px, 5vw) }", ".foo{width:rem(18px,5vw)}");
    minify_test(".foo { rotate: rem(-140deg, -90deg) }", ".foo{rotate:-50deg}");
    minify_test(".foo { rotate: rem(140deg, -90deg) }", ".foo{rotate:50deg}");
    minify_test(".foo { width: calc(10px * rem(18, 5)) }", ".foo{width:30px}");
    minify_test(".foo { width: mod(18px, 5px) }", ".foo{width:3px}");
    minify_test(".foo { width: mod(-18px, 5px) }", ".foo{width:2px}");
    minify_test(".foo { rotate: mod(-140deg, -90deg) }", ".foo{rotate:-50deg}");
    minify_test(".foo { rotate: mod(140deg, -90deg) }", ".foo{rotate:-40deg}");
    minify_test(".foo { width: mod(18px, 5vw) }", ".foo{width:mod(18px,5vw)}");
    minify_test(
      ".foo { transform: rotateX(mod(140deg, -90deg)) rotateY(rem(140deg, -90deg)) }",
      ".foo{transform:rotateX(-40deg)rotateY(50deg)}",
    );
    minify_test(".foo { width: calc(10px * mod(18, 5)) }", ".foo{width:30px}");

    minify_test(
      ".foo { width: calc(100% - 30px - 0) }",
      ".foo{width:calc(100% - 30px - 0)}",
    );
    minify_test(
      ".foo { width: calc(100% - 30px - 1 - 2) }",
      ".foo{width:calc(100% - 30px - 3)}",
    );
    minify_test(
      ".foo { width: calc(1 - 2 - 100% - 30px) }",
      ".foo{width:calc(-1 - 100% - 30px)}",
    );
    minify_test(
      ".foo { width: calc(2 * min(1px, 1vmin) - min(1px, 1vmin)); }",
      ".foo{width:calc(2*min(1px,1vmin) - min(1px,1vmin))}",
    );
    minify_test(
      ".foo { width: calc(100% - clamp(1.125rem, 1.25vw, 1.2375rem) - clamp(1.125rem, 1.25vw, 1.2375rem)); }",
      ".foo{width:calc(100% - clamp(1.125rem,1.25vw,1.2375rem) - clamp(1.125rem,1.25vw,1.2375rem))}",
    );
    minify_test(
      ".foo { width: calc(100% - 2 (2 * var(--card-margin))); }",
      ".foo{width:calc(100% - 2 (2*var(--card-margin)))}",
    );
  }

  #[test]
  fn test_trig() {
    minify_test(".foo { width: calc(2px * pi); }", ".foo{width:6.28319px}");
    minify_test(".foo { width: calc(2px / pi); }", ".foo{width:.63662px}");
    // minify_test(
    //   ".foo { width: calc(2px * infinity); }",
    //   ".foo{width:calc(2px*infinity)}",
    // );
    // minify_test(
    //   ".foo { width: calc(2px * -infinity); }",
    //   ".foo{width:calc(2px*-infinity)}",
    // );
    minify_test(".foo { width: calc(100px * sin(45deg))", ".foo{width:70.7107px}");
    minify_test(".foo { width: calc(100px * sin(.125turn))", ".foo{width:70.7107px}");
    minify_test(
      ".foo { width: calc(100px * sin(3.14159265 / 4))",
      ".foo{width:70.7107px}",
    );
    minify_test(".foo { width: calc(100px * sin(pi / 4))", ".foo{width:70.7107px}");
    minify_test(
      ".foo { width: calc(100px * sin(22deg + 23deg))",
      ".foo{width:70.7107px}",
    );

    minify_test(".foo { width: calc(2px * cos(45deg))", ".foo{width:1.41421px}");
    minify_test(".foo { width: calc(2px * tan(45deg))", ".foo{width:2px}");

    minify_test(".foo { rotate: asin(sin(45deg))", ".foo{rotate:45deg}");
    minify_test(".foo { rotate: asin(1)", ".foo{rotate:90deg}");
    minify_test(".foo { rotate: asin(-1)", ".foo{rotate:-90deg}");
    minify_test(".foo { rotate: asin(0.5)", ".foo{rotate:30deg}");
    minify_test(".foo { rotate: asin(45deg)", ".foo{rotate:asin(45deg)}"); // invalid
    minify_test(".foo { rotate: asin(-20)", ".foo{rotate:asin(-20)}"); // evaluates to NaN
    minify_test(".foo { width: asin(sin(45deg))", ".foo{width:asin(sin(45deg))}"); // invalid

    minify_test(".foo { rotate: acos(cos(45deg))", ".foo{rotate:45deg}");
    minify_test(".foo { rotate: acos(-1)", ".foo{rotate:180deg}");
    minify_test(".foo { rotate: acos(0)", ".foo{rotate:90deg}");
    minify_test(".foo { rotate: acos(1)", ".foo{rotate:none}");
    minify_test(".foo { rotate: acos(45deg)", ".foo{rotate:acos(45deg)}"); // invalid
    minify_test(".foo { rotate: acos(-20)", ".foo{rotate:acos(-20)}"); // evaluates to NaN

    minify_test(".foo { rotate: atan(tan(45deg))", ".foo{rotate:45deg}");
    minify_test(".foo { rotate: atan(1)", ".foo{rotate:45deg}");
    minify_test(".foo { rotate: atan(0)", ".foo{rotate:none}");
    minify_test(".foo { rotate: atan(45deg)", ".foo{rotate:atan(45deg)}"); // invalid

    minify_test(".foo { rotate: atan2(1px, -1px)", ".foo{rotate:135deg}");
    minify_test(".foo { rotate: atan2(1vw, -1vw)", ".foo{rotate:135deg}");
    minify_test(".foo { rotate: atan2(1, -1)", ".foo{rotate:135deg}");
    minify_test(".foo { rotate: atan2(1ms, -1ms)", ".foo{rotate:135deg}");
    minify_test(".foo { rotate: atan2(1%, -1%)", ".foo{rotate:135deg}");
    minify_test(".foo { rotate: atan2(1deg, -1deg)", ".foo{rotate:135deg}");
    minify_test(".foo { rotate: atan2(1cm, 1mm)", ".foo{rotate:84.2894deg}");
    minify_test(".foo { rotate: atan2(0, -1)", ".foo{rotate:180deg}");
    minify_test(".foo { rotate: atan2(-1, 1)", ".foo{rotate:-45deg}");
    // incompatible units
    minify_test(".foo { rotate: atan2(1px, -1vw)", ".foo{rotate:atan2(1px,-1vw)}");
  }

  #[test]
  fn test_exp() {
    minify_test(".foo { width: hypot()", ".foo{width:hypot()}");
    minify_test(".foo { width: hypot(1px)", ".foo{width:1px}");
    minify_test(".foo { width: hypot(1px, 2px)", ".foo{width:2.23607px}");
    minify_test(".foo { width: hypot(1px, 2px, 3px)", ".foo{width:3.74166px}");
    minify_test(".foo { width: hypot(1px, 2vw)", ".foo{width:hypot(1px,2vw)}");
    minify_test(".foo { width: hypot(1px, 2px, 3vw)", ".foo{width:hypot(1px,2px,3vw)}");
    minify_test(".foo { width: calc(100px * hypot(3, 4))", ".foo{width:500px}");
    minify_test(".foo { width: calc(1px * pow(2, sqrt(100))", ".foo{width:1024px}");
    minify_test(".foo { width: calc(100px * pow(2, pow(2, 2)", ".foo{width:1600px}");
    minify_test(".foo { width: calc(1px * log(1))", ".foo{width:0}");
    minify_test(".foo { width: calc(1px * log(10, 10))", ".foo{width:1px}");
    minify_test(".foo { width: calc(1px * exp(0))", ".foo{width:1px}");
    minify_test(".foo { width: calc(1px * log(e))", ".foo{width:1px}");
    minify_test(".foo { width: calc(1px * (e - exp(1)))", ".foo{width:0}");
    minify_test(
      ".foo { width: calc(1px * (exp(log(1) + exp(0)*2))",
      ".foo{width:7.38906px}",
    );
  }

  #[test]
  fn test_sign() {
    minify_test(".foo { width: abs(1px)", ".foo{width:1px}");
    minify_test(".foo { width: abs(-1px)", ".foo{width:1px}");
    minify_test(".foo { width: abs(1%)", ".foo{width:abs(1%)}"); // spec says percentages must be against resolved value

    minify_test(".foo { width: calc(10px * sign(-1vw)", ".foo{width:-10px}");
    minify_test(".foo { width: calc(10px * sign(1%)", ".foo{width:calc(10px*sign(1%))}");
  }

  #[test]
  fn test_box_shadow() {
    minify_test(
      ".foo { box-shadow: 64px 64px 12px 40px rgba(0,0,0,0.4) }",
      ".foo{box-shadow:64px 64px 12px 40px #0006}",
    );
    minify_test(
      ".foo { box-shadow: 12px 12px 0px 8px rgba(0,0,0,0.4) inset }",
      ".foo{box-shadow:inset 12px 12px 0 8px #0006}",
    );
    minify_test(
      ".foo { box-shadow: inset 12px 12px 0px 8px rgba(0,0,0,0.4) }",
      ".foo{box-shadow:inset 12px 12px 0 8px #0006}",
    );
    minify_test(
      ".foo { box-shadow: 12px 12px 8px 0px rgba(0,0,0,0.4) }",
      ".foo{box-shadow:12px 12px 8px #0006}",
    );
    minify_test(
      ".foo { box-shadow: 12px 12px 0px 0px rgba(0,0,0,0.4) }",
      ".foo{box-shadow:12px 12px #0006}",
    );
    minify_test(
      ".foo { box-shadow: 64px 64px 12px 40px rgba(0,0,0,0.4), 12px 12px 0px 8px rgba(0,0,0,0.4) inset }",
      ".foo{box-shadow:64px 64px 12px 40px #0006,inset 12px 12px 0 8px #0006}",
    );

    prefix_test(
      ".foo { box-shadow: 12px 12px lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          box-shadow: 12px 12px #b32323;
          box-shadow: 12px 12px lab(40% 56.6 39);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { box-shadow: 12px 12px lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          -webkit-box-shadow: 12px 12px #b32323;
          box-shadow: 12px 12px #b32323;
          box-shadow: 12px 12px lab(40% 56.6 39);
        }
      "#},
      Browsers {
        chrome: Some(4 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { box-shadow: 12px 12px lab(40% 56.6 39), 12px 12px yellow }",
      indoc! { r#"
        .foo {
          -webkit-box-shadow: 12px 12px #b32323, 12px 12px #ff0;
          box-shadow: 12px 12px #b32323, 12px 12px #ff0;
          box-shadow: 12px 12px lab(40% 56.6 39), 12px 12px #ff0;
        }
      "#},
      Browsers {
        chrome: Some(4 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { -webkit-box-shadow: 12px 12px #0006 }",
      indoc! { r#"
        .foo {
          -webkit-box-shadow: 12px 12px rgba(0, 0, 0, .4);
        }
      "#},
      Browsers {
        chrome: Some(4 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo {
        -webkit-box-shadow: 12px 12px #0006;
        -moz-box-shadow: 12px 12px #0009;
      }",
      indoc! { r#"
        .foo {
          -webkit-box-shadow: 12px 12px rgba(0, 0, 0, .4);
          -moz-box-shadow: 12px 12px rgba(0, 0, 0, .6);
        }
      "#},
      Browsers {
        chrome: Some(4 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo {
        -webkit-box-shadow: 12px 12px #0006;
        -moz-box-shadow: 12px 12px #0006;
        box-shadow: 12px 12px #0006;
      }",
      indoc! { r#"
        .foo {
          box-shadow: 12px 12px #0006;
        }
      "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { box-shadow: var(--foo) 12px lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          box-shadow: var(--foo) 12px #b32323;
        }

        @supports (color: lab(0% 0 0)) {
          .foo {
            box-shadow: var(--foo) 12px lab(40% 56.6 39);
          }
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        box-shadow: 0px 0px 22px red;
        box-shadow: 0px 0px max(2cqw, 22px) red;
      }
    "#,
      indoc! {r#"
      .foo {
        box-shadow: 0 0 22px red;
        box-shadow: 0 0 max(2cqw, 22px) red;
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        box-shadow: 0px 0px 22px red;
        box-shadow: 0px 0px max(2cqw, 22px) red;
      }
    "#,
      indoc! {r#"
      .foo {
        box-shadow: 0 0 max(2cqw, 22px) red;
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        box-shadow: 0px 0px 22px red;
        box-shadow: 0px 0px 22px lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        box-shadow: 0 0 22px red;
        box-shadow: 0 0 22px lab(40% 56.6 39);
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        box-shadow: 0px 0px 22px red;
        box-shadow: 0px 0px 22px lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        box-shadow: 0 0 22px lab(40% 56.6 39);
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        box-shadow: var(--fallback);
        box-shadow: 0px 0px 22px lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        box-shadow: var(--fallback);
        box-shadow: 0 0 22px lab(40% 56.6 39);
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_media() {
    minify_test(
      "@media (min-width: 240px) { .foo { color: chartreuse }}",
      "@media (width>=240px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (width < 240px) { .foo { color: chartreuse }}",
      "@media (width<240px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (width <= 240px) { .foo { color: chartreuse }}",
      "@media (width<=240px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (width > 240px) { .foo { color: chartreuse }}",
      "@media (width>240px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (width >= 240px) { .foo { color: chartreuse }}",
      "@media (width>=240px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (240px < width) { .foo { color: chartreuse }}",
      "@media (width>240px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (240px <= width) { .foo { color: chartreuse }}",
      "@media (width>=240px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (240px > width) { .foo { color: chartreuse }}",
      "@media (width<240px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (240px >= width) { .foo { color: chartreuse }}",
      "@media (width<=240px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (100px < width < 200px) { .foo { color: chartreuse }}",
      "@media (100px<width<200px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (100px <= width <= 200px) { .foo { color: chartreuse }}",
      "@media (100px<=width<=200px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (min-width: 30em) and (max-width: 50em) { .foo { color: chartreuse }}",
      "@media (width>=30em) and (width<=50em){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media screen, print { .foo { color: chartreuse }}",
      "@media screen,print{.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (hover: hover) { .foo { color: chartreuse }}",
      "@media (hover:hover){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (hover) { .foo { color: chartreuse }}",
      "@media (hover){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (aspect-ratio: 11/5) { .foo { color: chartreuse }}",
      "@media (aspect-ratio:11/5){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (aspect-ratio: 2/1) { .foo { color: chartreuse }}",
      "@media (aspect-ratio:2){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (aspect-ratio: 2) { .foo { color: chartreuse }}",
      "@media (aspect-ratio:2){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media not screen and (color) { .foo { color: chartreuse }}",
      "@media not screen and (color){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media only screen and (color) { .foo { color: chartreuse }}",
      "@media only screen and (color){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (update: slow) or (hover: none) { .foo { color: chartreuse }}",
      "@media (update:slow) or (hover:none){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (width < 600px) and (height < 600px) { .foo { color: chartreuse }}",
      "@media (width<600px) and (height<600px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (not (color)) or (hover) { .foo { color: chartreuse }}",
      "@media (not (color)) or (hover){.foo{color:#7fff00}}",
    );
    error_test(
      "@media (example, all,), speech { .foo { color: chartreuse }}",
      ParserError::UnexpectedToken(Token::Comma),
    );
    error_test(
      "@media &test, speech { .foo { color: chartreuse }}",
      ParserError::UnexpectedToken(Token::Delim('&')),
    );
    error_test(
      "@media &test { .foo { color: chartreuse }}",
      ParserError::UnexpectedToken(Token::Delim('&')),
    );
    minify_test(
      "@media (min-width: calc(200px + 40px)) { .foo { color: chartreuse }}",
      "@media (width>=240px){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (min-width: calc(1em + 5px)) { .foo { color: chartreuse }}",
      "@media (width>=calc(1em + 5px)){.foo{color:#7fff00}}",
    );
    minify_test("@media { .foo { color: chartreuse }}", ".foo{color:#7fff00}");
    minify_test("@media all { .foo { color: chartreuse }}", ".foo{color:#7fff00}");
    minify_test(
      "@media not (((color) or (hover))) { .foo { color: chartreuse }}",
      "@media not ((color) or (hover)){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (hover) and ((color) and (test)) { .foo { color: chartreuse }}",
      "@media (hover) and (color) and (test){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (grid: 1) { .foo { color: chartreuse }}",
      "@media (grid:1){.foo{color:#7fff00}}",
    );
    minify_test(
      "@media (width >= calc(2px + 4px)) { .foo { color: chartreuse }}",
      "@media (width>=6px){.foo{color:#7fff00}}",
    );

    prefix_test(
      r#"
        @media (width >= 240px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (min-width: 240px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(60 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (width >= 240px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (width >= 240px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(64 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (color > 2) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media not (max-color: 2) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(60 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (color < 2) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media not (min-color: 2) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(60 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (width > 240px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media not (max-width: 240px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(60 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (width <= 240px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (max-width: 240px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(60 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (width <= 240px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (width <= 240px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(64 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (width < 240px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media not (min-width: 240px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(60 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media not (width < 240px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (min-width: 240px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(60 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
        @media not (width < 240px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (width >= 240px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
    );

    prefix_test(
      r#"
        @media (width < 240px) and (hover) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (not (min-width: 240px)) and (hover) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(60 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (100px <= width <= 200px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (min-width: 100px) and (max-width: 200px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media not (100px <= width <= 200px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media not ((min-width: 100px) and (max-width: 200px)) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (hover) and (100px <= width <= 200px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (hover) and (min-width: 100px) and (max-width: 200px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (hover) or (100px <= width <= 200px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (hover) or ((min-width: 100px) and (max-width: 200px)) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (100px < width < 200px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (not (max-width: 100px)) and (not (min-width: 200px)) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media not (100px < width < 200px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media not ((not (max-width: 100px)) and (not (min-width: 200px))) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        @media (200px >= width >= 100px) {
          .foo {
            color: chartreuse;
          }
        }
      "#,
      indoc! { r#"
        @media (max-width: 200px) and (min-width: 100px) {
          .foo {
            color: #7fff00;
          }
        }
      "#},
      Browsers {
        firefox: Some(85 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
      @media not all {
        .a {
          color: green;
        }
      }
      "#,
      "\n",
    );

    prefix_test(
      r#"
      @media (width > calc(1px + 1rem)) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media not (max-width: calc(1px + 1rem)) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        chrome: Some(85 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @media (width > max(10px, 1rem)) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media not (max-width: max(10px, 1rem)) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        chrome: Some(85 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @media (width > 0) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media not (max-width: 0) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        chrome: Some(85 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @media (min-resolution: 2dppx) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media (-webkit-min-device-pixel-ratio: 2), (min-resolution: 2dppx) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @media (min-resolution: 2dppx) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media (min--moz-device-pixel-ratio: 2), (min-resolution: 2dppx) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        firefox: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @media (resolution > 2dppx) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media not (-webkit-max-device-pixel-ratio: 2), not (max-resolution: 2dppx) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @media (resolution >= 300dpi) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media (-webkit-min-device-pixel-ratio: 3.125), (min-resolution: 300dpi) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @media (min-resolution: 113.38dpcm) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media (-webkit-min-device-pixel-ratio: 2.99985), (min--moz-device-pixel-ratio: 2.99985), (min-resolution: 113.38dpcm) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        firefox: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @media (color) and (min-resolution: 2dppx) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media (color) and (-webkit-min-device-pixel-ratio: 2), (color) and (min-resolution: 2dppx) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @media (min-resolution: 2dppx),
             (min-resolution: 192dpi) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media (-webkit-min-device-pixel-ratio: 2), (min--moz-device-pixel-ratio: 2), (min-resolution: 2dppx), (min-resolution: 192dpi) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        firefox: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @media only screen and (min-resolution: 124.8dpi) {
        .foo { color: yellow; }
      }
      "#,
      indoc! { r#"
        @media only screen and (-webkit-min-device-pixel-ratio: 1.3), only screen and (min--moz-device-pixel-ratio: 1.3), only screen and (min-resolution: 124.8dpi) {
          .foo {
            color: #ff0;
          }
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        firefox: Some(10 << 16),
        ..Browsers::default()
      },
    );

    error_test(
      "@media (min-width: hi) { .foo { color: chartreuse }}",
      ParserError::InvalidMediaQuery,
    );
    error_test(
      "@media (width >= hi) { .foo { color: chartreuse }}",
      ParserError::InvalidMediaQuery,
    );
    error_test(
      "@media (width >= 2/1) { .foo { color: chartreuse }}",
      ParserError::UnexpectedToken(Token::Delim('/')),
    );
    error_test(
      "@media (600px <= min-height) { .foo { color: chartreuse }}",
      ParserError::InvalidMediaQuery,
    );
    error_test(
      "@media (scan >= 1) { .foo { color: chartreuse }}",
      ParserError::InvalidMediaQuery,
    );
    error_test(
      "@media (min-scan: interlace) { .foo { color: chartreuse }}",
      ParserError::InvalidMediaQuery,
    );
    error_test(
      "@media (1px <= width <= bar) { .foo { color: chartreuse }}",
      ParserError::InvalidMediaQuery,
    );
    error_test(
      "@media (1px <= min-width <= 2px) { .foo { color: chartreuse }}",
      ParserError::InvalidMediaQuery,
    );
    error_test(
      "@media (1px <= scan <= 2px) { .foo { color: chartreuse }}",
      ParserError::InvalidMediaQuery,
    );
    error_test(
      "@media (grid: 10) { .foo { color: chartreuse }}",
      ParserError::InvalidMediaQuery,
    );
    error_test(
      "@media (prefers-color-scheme = dark) { .foo { color: chartreuse }}",
      ParserError::InvalidMediaQuery,
    );
  }

  #[test]
  fn test_merge_layers() {
    test(
      r#"
      @layer foo {
        .foo {
          color: red;
        }
      }
      @layer foo {
        .foo {
          background: #fff;
        }

        .baz {
          color: #fff;
        }
      }
      "#,
      indoc! {r#"
      @layer foo {
        .foo {
          color: red;
          background: #fff;
        }

        .baz {
          color: #fff;
        }
      }
    "#},
    );
    test(
      r#"
      @layer a {}
      @layer b {}

      @layer b {
        foo {
          color: red;
        }
      }

      @layer a {
        bar {
          color: yellow;
        }
      }
      "#,
      indoc! {r#"
      @layer a {
        bar {
          color: #ff0;
        }
      }

      @layer b {
        foo {
          color: red;
        }
      }
    "#},
    );

    test(
      r#"
      @layer a {}
      @layer b {}

      @layer b {
        foo {
          color: red;
        }
      }
      "#,
      indoc! {r#"
      @layer a;

      @layer b {
        foo {
          color: red;
        }
      }
    "#},
    );

    test(
      r#"
      @layer a;
      @layer b;
      @layer c;
      "#,
      indoc! {r#"
      @layer a, b, c;
    "#},
    );

    test(
      r#"
      @layer a {}
      @layer b {}
      @layer c {}
      "#,
      indoc! {r#"
      @layer a, b, c;
    "#},
    );

    test(
      r#"
      @layer a;
      @layer b {
        .foo {
          color: red;
        }
      }
      @layer c {}
      "#,
      indoc! {r#"
      @layer a;

      @layer b {
        .foo {
          color: red;
        }
      }

      @layer c;
    "#},
    );

    test(
      r#"
      @layer a, b;
      @layer c {}

      @layer d {
        foo {
          color: red;
        }
      }
      "#,
      indoc! {r#"
      @layer a, b, c;

      @layer d {
        foo {
          color: red;
        }
      }
    "#},
    );

    test(
      r#"
      @layer a;
      @layer b;
      @import "a.css" layer(x);
      @layer c;

      @layer d {
        foo {
          color: red;
        }
      }
      "#,
      indoc! {r#"
      @layer a, b;
      @import "a.css" layer(x);
      @layer c;

      @layer d {
        foo {
          color: red;
        }
      }
    "#},
    );

    test(
      r#"
      @layer a, b, c;

      @layer a {
        foo {
          color: red;
        }
      }
      "#,
      indoc! {r#"
      @layer a {
        foo {
          color: red;
        }
      }

      @layer b, c;
    "#},
    );
  }

  #[test]
  fn test_merge_rules() {
    test(
      r#"
      .foo {
        color: red;
      }
      .bar {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo, .bar {
        color: red;
      }
    "#},
    );
    test(
      r#"
      .foo {
        color: red;
      }
      .foo {
        background: green;
      }
    "#,
      indoc! {r#"
      .foo {
        color: red;
        background: green;
      }
    "#},
    );
    test(
      r#"
      .foo {
        color: red;
      }
      .foo {
        background: green !important;
      }
    "#,
      indoc! {r#"
      .foo {
        color: red;
        background: green !important;
      }
    "#},
    );
    test(
      r#"
      .foo {
        background: red;
      }
      .foo {
        background: green;
      }
    "#,
      indoc! {r#"
      .foo {
        background: green;
      }
    "#},
    );
    test(
      r#"
      .foo {
        --foo: red;
        --foo: purple;
      }
      .foo {
        --foo: green;
      }
    "#,
      indoc! {r#"
      .foo {
        --foo: green;
      }
    "#},
    );
    test(
      r#"
      .foo {
        color: red;
      }

      .bar {
        background: green;
      }
    "#,
      indoc! {r#"
      .foo {
        color: red;
      }

      .bar {
        background: green;
      }
    "#},
    );
    test(
      r#"
      .foo {
        color: red;
      }

      .baz {
        color: blue;
      }

      .bar {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo {
        color: red;
      }

      .baz {
        color: #00f;
      }

      .bar {
        color: red;
      }
    "#},
    );
    test(
      r#"
      .foo {
        background: red;
      }
      .bar {
        background: red;
      }
      .foo {
        color: green;
      }
      .bar {
        color: green;
      }
    "#,
      indoc! {r#"
      .foo, .bar {
        color: green;
        background: red;
      }
    "#},
    );
    test(
      r#"
      .foo, .bar {
        background: red;
      }
      .foo {
        color: green;
      }
      .bar {
        color: green;
      }
    "#,
      indoc! {r#"
      .foo, .bar {
        color: green;
        background: red;
      }
    "#},
    );
    test(
      r#"
      .foo {
        background: red;
      }
      .foo {
        color: green;
      }
      .bar {
        background: red;
      }
      .bar {
        color: green;
      }
    "#,
      indoc! {r#"
      .foo, .bar {
        color: green;
        background: red;
      }
    "#},
    );
    test(
      r#"
      [foo="bar"] {
        color: red;
      }
      .bar {
        color: red;
      }
    "#,
      indoc! {r#"
      [foo="bar"], .bar {
        color: red;
      }
    "#},
    );
    test(
      r#"
      .a {
        color: red;
      }
      .b {
        color: green;
      }
      .a {
        color: red;
      }
    "#,
      indoc! {r#"
      .b {
        color: green;
      }

      .a {
        color: red;
      }
    "#},
    );
    test(
      r#"
      .a {
        color: red;
      }
      .b {
        color: green;
      }
      .a {
        color: pink;
      }
    "#,
      indoc! {r#"
      .b {
        color: green;
      }

      .a {
        color: pink;
      }
    "#},
    );
    test(
      r#"
      .a:foo(#000) {
        color: red;
      }
      .b {
        color: green;
      }
      .a:foo(#ff0) {
        color: pink;
      }
    "#,
      indoc! {r#"
      .a:foo(#000) {
        color: red;
      }

      .b {
        color: green;
      }

      .a:foo(#ff0) {
        color: pink;
      }
    "#},
    );
    test(
      r#"
      .a {
        border-radius: 10px;
      }
      .b {
        color: green;
      }
      .a {
        border-radius: 10px;
      }
    "#,
      indoc! {r#"
      .b {
        color: green;
      }

      .a {
        border-radius: 10px;
      }
    "#},
    );
    test(
      r#"
      .a {
        border-radius: 10px;
      }
      .b {
        color: green;
      }
      .a {
        -webkit-border-radius: 10px;
      }
    "#,
      indoc! {r#"
      .a {
        border-radius: 10px;
      }

      .b {
        color: green;
      }

      .a {
        -webkit-border-radius: 10px;
      }
    "#},
    );
    test(
      r#"
      .a {
        border-radius: 10px;
      }
      .b {
        color: green;
      }
      .a {
        border-radius: var(--foo);
      }
    "#,
      indoc! {r#"
      .b {
        color: green;
      }

      .a {
        border-radius: var(--foo);
      }
    "#},
    );
    test(
      r#"
      .a {
        border-radius: 10px;
      }
      .b {
        color: green;
      }
      .c {
        border-radius: 20px;
      }
    "#,
      indoc! {r#"
      .a {
        border-radius: 10px;
      }

      .b {
        color: green;
      }

      .c {
        border-radius: 20px;
      }
    "#},
    );
    test(
      r#"
      @media print {
        .a {
          color: red;
        }
        .b {
          color: green;
        }
        .a {
          color: red;
        }
      }
    "#,
      indoc! {r#"
      @media print {
        .b {
          color: green;
        }

        .a {
          color: red;
        }
      }
    "#},
    );
    test(
      r#"
      .a {
        border-radius: 10px;
      }
      .b {
        color: green;
      }
      .a {
        border-radius: 20px;
        color: pink;
      }
    "#,
      indoc! {r#"
      .a {
        border-radius: 10px;
      }

      .b {
        color: green;
      }

      .a {
        color: pink;
        border-radius: 20px;
      }
    "#},
    );
    test(
      r#"
      .a {
        color: red;
      }
      .b {
        color: green;
      }
      .a {
        color: red;
      }
      .b {
        color: green;
      }
    "#,
      indoc! {r#"
      .a {
        color: red;
      }

      .b {
        color: green;
      }
    "#},
    );

    prefix_test(
      r#"
      [foo="bar"] {
        color: red;
      }
      .bar {
        color: red;
      }
    "#,
      indoc! {r#"
      [foo="bar"] {
        color: red;
      }

      .bar {
        color: red;
      }
    "#},
      Browsers {
        ie: Some(6 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      [foo="bar"] {
        color: red;
      }
      .bar {
        color: red;
      }
    "#,
      indoc! {r#"
      [foo="bar"], .bar {
        color: red;
      }
    "#},
      Browsers {
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
      .foo:-moz-read-only {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo:-moz-read-only {
        color: red;
      }
    "#},
    );

    test(
      r#"
      .foo:-moz-read-only {
        color: red;
      }

      .foo:read-only {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo:-moz-read-only {
        color: red;
      }

      .foo:read-only {
        color: red;
      }
    "#},
    );

    prefix_test(
      r#"
      .foo:-moz-read-only {
        color: red;
      }

      .foo:read-only {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo:read-only {
        color: red;
      }
    "#},
      Browsers {
        firefox: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:-moz-read-only {
        color: red;
      }

      .bar {
        color: yellow;
      }

      .foo:read-only {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo:-moz-read-only {
        color: red;
      }

      .bar {
        color: #ff0;
      }

      .foo:read-only {
        color: red;
      }
    "#},
      Browsers {
        firefox: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:-moz-read-only {
        color: red;
      }

      .foo:read-only {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo:-moz-read-only {
        color: red;
      }

      .foo:read-only {
        color: red;
      }
    "#},
      Browsers {
        firefox: Some(36 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:read-only {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo:-moz-read-only {
        color: red;
      }

      .foo:read-only {
        color: red;
      }
    "#},
      Browsers {
        firefox: Some(36 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:-webkit-full-screen {
        color: red;
      }
      .foo:-moz-full-screen {
        color: red;
      }
      .foo:-ms-fullscreen {
        color: red;
      }
      .foo:fullscreen {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo:fullscreen {
        color: red;
      }
    "#},
      Browsers {
        chrome: Some(96 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:fullscreen {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo:-webkit-full-screen {
        color: red;
      }

      .foo:-moz-full-screen {
        color: red;
      }

      .foo:-ms-fullscreen {
        color: red;
      }

      .foo:fullscreen {
        color: red;
      }
    "#},
      Browsers {
        chrome: Some(45 << 16),
        firefox: Some(45 << 16),
        ie: Some(11 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo::placeholder {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo::-webkit-input-placeholder {
        color: red;
      }

      .foo::-moz-placeholder {
        color: red;
      }

      .foo::-ms-input-placeholder {
        color: red;
      }

      .foo::placeholder {
        color: red;
      }
    "#},
      Browsers {
        chrome: Some(45 << 16),
        firefox: Some(45 << 16),
        ie: Some(11 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo::file-selector-button {
        color: red;
      }
    "#,
      indoc! {r#"
      .foo::-webkit-file-upload-button {
        color: red;
      }

      .foo::-ms-browse {
        color: red;
      }

      .foo::file-selector-button {
        color: red;
      }
    "#},
      Browsers {
        chrome: Some(84 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo::file-selector-button {
        margin-inline-start: 2px;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)))::-webkit-file-upload-button {
        margin-left: 2px;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)))::-ms-browse {
        margin-left: 2px;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)))::file-selector-button {
        margin-left: 2px;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))::-webkit-file-upload-button {
        margin-right: 2px;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))::-ms-browse {
        margin-right: 2px;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))::file-selector-button {
        margin-right: 2px;
      }
    "#},
      Browsers {
        chrome: Some(84 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:placeholder-shown .bar { color: red; }
      .foo:autofill .baz { color: red; }
      "#,
      indoc! {r#"
      .foo:placeholder-shown .bar {
        color: red;
      }

      .foo:-webkit-autofill .baz {
        color: red;
      }

      .foo:autofill .baz {
        color: red;
      }
      "#},
      Browsers {
        chrome: Some(103 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:placeholder-shown .bar,.foo:autofill .baz{color:red}
      "#,
      indoc! {r#"
      :-webkit-any(.foo:placeholder-shown .bar, .foo:-webkit-autofill .baz) {
        color: red;
      }

      :is(.foo:placeholder-shown .bar, .foo:autofill .baz) {
        color: red;
      }
      "#},
      Browsers {
        chrome: Some(103 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:placeholder-shown .bar, .foo:-webkit-autofill .baz {
        color: red;
      }

      .foo:placeholder-shown .bar, .foo:autofill .baz {
        color: red;
      }
      "#,
      indoc! {r#"
      :-webkit-any(.foo:placeholder-shown .bar, .foo:-webkit-autofill .baz) {
        color: red;
      }

      :is(.foo:placeholder-shown .bar, .foo:autofill .baz) {
        color: red;
      }
      "#},
      Browsers {
        chrome: Some(103 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
      .foo:placeholder-shown .bar, .foo:-webkit-autofill .baz {
        color: red;
      }

      .foo:placeholder-shown .bar, .foo:autofill .baz {
        color: red;
      }
      "#,
      indoc! {r#"
      .foo:placeholder-shown .bar, .foo:-webkit-autofill .baz {
        color: red;
      }

      .foo:placeholder-shown .bar, .foo:autofill .baz {
        color: red;
      }
      "#},
    );

    prefix_test(
      r#"
      :hover, :focus-visible {
        color: red;
      }
      "#,
      indoc! {r#"
      :hover {
        color: red;
      }

      :focus-visible {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        color: red;
      }

      :hover, :focus-visible {
        color: red;
      }
      "#,
      indoc! {r#"
      .foo, :hover {
        color: red;
      }

      :focus-visible {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      :hover, :focus-visible {
        margin-inline-start: 24px;
      }
      "#,
      indoc! {r#"
      :hover:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        margin-left: 24px;
      }

      :hover:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        margin-right: 24px;
      }

      :focus-visible:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        margin-left: 24px;
      }

      :focus-visible:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        margin-right: 24px;
      }
      "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      :focus-within, :focus-visible {
        color: red;
      }
      "#,
      indoc! {r#"
      :focus-within {
        color: red;
      }

      :focus-visible {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(9 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      :hover, :focus-visible {
        color: red;
      }
      "#,
      indoc! {r#"
      :is(:hover, :focus-visible) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      a::after:hover, a::after:focus-visible {
        color: red;
      }
      "#,
      indoc! {r#"
      a:after:hover {
        color: red;
      }

      a:after:focus-visible {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      a:not(:hover), a:not(:focus-visible) {
        color: red;
      }
      "#,
      indoc! {r#"
      :is(a:not(:hover), a:not(:focus-visible)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      a:has(:hover), a:has(:focus-visible) {
        color: red;
      }
      "#,
      indoc! {r#"
      :is(a:has(:hover), a:has(:focus-visible)) {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo.foo:hover, .bar:focus-visible {
        color: red;
      }
      "#,
      indoc! {r#"
      .foo.foo:hover {
        color: red;
      }

      .bar:focus-visible {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      a::unknown-a, a::unknown-b {
        color: red;
      }
      "#,
      indoc! {r#"
      a::unknown-a {
        color: red;
      }

      a::unknown-b {
        color: red;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    nesting_test_with_targets(
      r#"
      .foo {
        padding-inline-start: 3px;

        .bar {
          padding-inline-start: 5px;
        }
      }
      "#,
      indoc! {r#"
      .foo:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        padding-left: 3px;
      }

      .foo:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        padding-right: 3px;
      }

      .foo .bar:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        padding-left: 5px;
      }

      .foo .bar:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        padding-right: 5px;
      }
      "#},
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      }
      .into(),
    );

    prefix_test(
      r#"
      .foo::part(header), .foo::part(body) {
        display: none
      }
      "#,
      indoc! {r#"
      .foo::part(header), .foo::part(body) {
        display: none;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo :is(.bar) {
        color: red;
      }
      "#,
      indoc! {r#"
        .foo .bar {
          color: red;
        }
      "#},
      Browsers {
        chrome: Some(87 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo :is(.bar), .bar :is(.baz) {
        color: red;
      }
      "#,
      indoc! {r#"
        .foo .bar, .bar .baz {
          color: red;
        }
      "#},
      Browsers {
        chrome: Some(87 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo :is(.bar:focus-visible), .bar :is(.baz:hover) {
        color: red;
      }
      "#,
      indoc! {r#"
        .bar .baz:hover {
          color: red;
        }

        .foo .bar:focus-visible {
          color: red;
        }
      "#},
      Browsers {
        chrome: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      *,
      ::before,
      ::after,
      ::backdrop {
        padding: 5px;
      }
      "#,
      indoc! {r#"
        *, :before, :after {
          padding: 5px;
        }

        ::-webkit-backdrop {
          padding: 5px;
        }

        ::backdrop {
          padding: 5px;
        }
      "#},
      Browsers {
        chrome: Some(33 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
      .foo:-webkit-any(.bar, .baz):after {
        color: red;
      }

      .foo:is(.bar, .baz):after {
        color: red;
      }
      "#,
      indoc! {r#"
        .foo:-webkit-any(.bar, .baz):after {
          color: red;
        }

        .foo:is(.bar, .baz):after {
          color: red;
        }
      "#},
    );

    prefix_test(
      r#"
      .foo:-webkit-any(.bar, .baz):after {
        color: red;
      }

      .foo:is(.bar, .baz):after {
        color: red;
      }
      "#,
      indoc! {r#"
        .foo:is(.bar, .baz):after {
          color: red;
        }
      "#},
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:-webkit-any(.bar):after {
        color: red;
      }

      .foo:is(.bar, .baz):after {
        color: red;
      }
      "#,
      indoc! {r#"
        .foo:-webkit-any(.bar):after {
          color: red;
        }

        .foo:is(.bar, .baz):after {
          color: red;
        }
      "#},
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:-webkit-any(.bar, .baz):after {
        color: red;
      }

      .foo:is(.bar, .baz):after {
        color: red;
      }
      "#,
      indoc! {r#"
        .foo:-webkit-any(.bar, .baz):after {
          color: red;
        }

        .foo:is(.bar, .baz):after {
          color: red;
        }
      "#},
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:-webkit-any(.bar, .baz):after {
        color: red;
      }

      .foo:-moz-any(.bar, .baz):after {
        color: red;
      }
      "#,
      indoc! {r#"
        .foo:-webkit-any(.bar, .baz):after {
          color: red;
        }

        .foo:-moz-any(.bar, .baz):after {
          color: red;
        }
      "#},
      Browsers {
        safari: Some(12 << 16),
        firefox: Some(67 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .a {
        padding-inline: var(--foo);
      }

      .a:-webkit-any(.b, .c) {
        padding-inline: var(--foo);
      }
      "#,
      indoc! {r#"
        .a {
          padding-inline: var(--foo);
        }

        .a:-webkit-any(.b, .c) {
          padding-inline: var(--foo);
        }
      "#},
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_merge_media_rules() {
    test(
      r#"
      @media (hover) {
        .foo {
          color: red;
        }
      }
      @media (hover) {
        .foo {
          background: #fff;
        }

        .baz {
          color: #fff;
        }
      }
      "#,
      indoc! {r#"
      @media (hover) {
        .foo {
          color: red;
          background: #fff;
        }

        .baz {
          color: #fff;
        }
      }
    "#},
    );

    test(
      r#"
      @media (hover) {
        .foo {
          color: red;
        }
      }
      @media (min-width: 250px) {
        .foo {
          background: #fff;
        }

        .baz {
          color: #fff;
        }
      }
      "#,
      indoc! {r#"
      @media (hover) {
        .foo {
          color: red;
        }
      }

      @media (width >= 250px) {
        .foo {
          background: #fff;
        }

        .baz {
          color: #fff;
        }
      }
    "#},
    );
  }

  #[test]
  fn test_merge_supports() {
    test(
      r#"
      @supports (flex: 1) {
        .foo {
          color: red;
        }
      }
      @supports (flex: 1) {
        .foo {
          background: #fff;
        }

        .baz {
          color: #fff;
        }
      }
      "#,
      indoc! {r#"
      @supports (flex: 1) {
        .foo {
          color: red;
          background: #fff;
        }

        .baz {
          color: #fff;
        }
      }
    "#},
    );

    test(
      r#"
      @supports (flex: 1) {
        .foo {
          color: red;
        }
      }
      @supports (display: grid) {
        .foo {
          background: #fff;
        }

        .baz {
          color: #fff;
        }
      }
      "#,
      indoc! {r#"
      @supports (flex: 1) {
        .foo {
          color: red;
        }
      }

      @supports (display: grid) {
        .foo {
          background: #fff;
        }

        .baz {
          color: #fff;
        }
      }
    "#},
    );
  }

  #[test]
  fn test_opacity() {
    minify_test(".foo { opacity: 0 }", ".foo{opacity:0}");
    minify_test(".foo { opacity: 0% }", ".foo{opacity:0}");
    minify_test(".foo { opacity: 0.5 }", ".foo{opacity:.5}");
    minify_test(".foo { opacity: 50% }", ".foo{opacity:.5}");
    minify_test(".foo { opacity: 1 }", ".foo{opacity:1}");
    minify_test(".foo { opacity: 100% }", ".foo{opacity:1}");
  }

  #[test]
  fn test_transitions() {
    minify_test(".foo { transition-duration: 500ms }", ".foo{transition-duration:.5s}");
    minify_test(".foo { transition-duration: .5s }", ".foo{transition-duration:.5s}");
    minify_test(".foo { transition-duration: 99ms }", ".foo{transition-duration:99ms}");
    minify_test(".foo { transition-duration: .099s }", ".foo{transition-duration:99ms}");
    minify_test(".foo { transition-duration: 2000ms }", ".foo{transition-duration:2s}");
    minify_test(".foo { transition-duration: 2s }", ".foo{transition-duration:2s}");
    minify_test(
      ".foo { transition-duration: calc(1s - 50ms) }",
      ".foo{transition-duration:.95s}",
    );
    minify_test(
      ".foo { transition-duration: calc(1s - 50ms + 2s) }",
      ".foo{transition-duration:2.95s}",
    );
    minify_test(
      ".foo { transition-duration: calc((1s - 50ms) * 2) }",
      ".foo{transition-duration:1.9s}",
    );
    minify_test(
      ".foo { transition-duration: calc(2 * (1s - 50ms)) }",
      ".foo{transition-duration:1.9s}",
    );
    minify_test(
      ".foo { transition-duration: calc((2s + 50ms) - (1s - 50ms)) }",
      ".foo{transition-duration:1.1s}",
    );
    minify_test(
      ".foo { transition-duration: 500ms, 50ms }",
      ".foo{transition-duration:.5s,50ms}",
    );
    minify_test(".foo { transition-delay: 500ms }", ".foo{transition-delay:.5s}");
    minify_test(
      ".foo { transition-property: background }",
      ".foo{transition-property:background}",
    );
    minify_test(
      ".foo { transition-property: background, opacity }",
      ".foo{transition-property:background,opacity}",
    );
    minify_test(
      ".foo { transition-timing-function: linear }",
      ".foo{transition-timing-function:linear}",
    );
    minify_test(
      ".foo { transition-timing-function: ease }",
      ".foo{transition-timing-function:ease}",
    );
    minify_test(
      ".foo { transition-timing-function: ease-in }",
      ".foo{transition-timing-function:ease-in}",
    );
    minify_test(
      ".foo { transition-timing-function: ease-out }",
      ".foo{transition-timing-function:ease-out}",
    );
    minify_test(
      ".foo { transition-timing-function: ease-in-out }",
      ".foo{transition-timing-function:ease-in-out}",
    );
    minify_test(
      ".foo { transition-timing-function: cubic-bezier(0.25, 0.1, 0.25, 1) }",
      ".foo{transition-timing-function:ease}",
    );
    minify_test(
      ".foo { transition-timing-function: cubic-bezier(0.42, 0, 1, 1) }",
      ".foo{transition-timing-function:ease-in}",
    );
    minify_test(
      ".foo { transition-timing-function: cubic-bezier(0, 0, 0.58, 1) }",
      ".foo{transition-timing-function:ease-out}",
    );
    minify_test(
      ".foo { transition-timing-function: cubic-bezier(0.42, 0, 0.58, 1) }",
      ".foo{transition-timing-function:ease-in-out}",
    );
    minify_test(
      ".foo { transition-timing-function: cubic-bezier(0.58, 0.2, 0.11, 1.2) }",
      ".foo{transition-timing-function:cubic-bezier(.58,.2,.11,1.2)}",
    );
    minify_test(
      ".foo { transition-timing-function: step-start }",
      ".foo{transition-timing-function:step-start}",
    );
    minify_test(
      ".foo { transition-timing-function: step-end }",
      ".foo{transition-timing-function:step-end}",
    );
    minify_test(
      ".foo { transition-timing-function: steps(1, start) }",
      ".foo{transition-timing-function:step-start}",
    );
    minify_test(
      ".foo { transition-timing-function: steps(1, jump-start) }",
      ".foo{transition-timing-function:step-start}",
    );
    minify_test(
      ".foo { transition-timing-function: steps(1, end) }",
      ".foo{transition-timing-function:step-end}",
    );
    minify_test(
      ".foo { transition-timing-function: steps(1, jump-end) }",
      ".foo{transition-timing-function:step-end}",
    );
    minify_test(
      ".foo { transition-timing-function: steps(5, jump-start) }",
      ".foo{transition-timing-function:steps(5,start)}",
    );
    minify_test(
      ".foo { transition-timing-function: steps(5, jump-end) }",
      ".foo{transition-timing-function:steps(5,end)}",
    );
    minify_test(
      ".foo { transition-timing-function: steps(5, jump-both) }",
      ".foo{transition-timing-function:steps(5,jump-both)}",
    );
    minify_test(
      ".foo { transition-timing-function: ease-in-out, cubic-bezier(0.42, 0, 1, 1) }",
      ".foo{transition-timing-function:ease-in-out,ease-in}",
    );
    minify_test(
      ".foo { transition-timing-function: cubic-bezier(0.42, 0, 1, 1), cubic-bezier(0.58, 0.2, 0.11, 1.2) }",
      ".foo{transition-timing-function:ease-in,cubic-bezier(.58,.2,.11,1.2)}",
    );
    minify_test(
      ".foo { transition-timing-function: step-start, steps(5, jump-start) }",
      ".foo{transition-timing-function:step-start,steps(5,start)}",
    );
    minify_test(".foo { transition: width 2s ease }", ".foo{transition:width 2s}");
    minify_test(
      ".foo { transition: width 2s ease, height 1000ms cubic-bezier(0.25, 0.1, 0.25, 1) }",
      ".foo{transition:width 2s,height 1s}",
    );
    minify_test(".foo { transition: width 2s 1s }", ".foo{transition:width 2s 1s}");
    minify_test(".foo { transition: width 2s ease 1s }", ".foo{transition:width 2s 1s}");
    minify_test(
      ".foo { transition: ease-in 1s width 4s }",
      ".foo{transition:width 1s ease-in 4s}",
    );
    minify_test(".foo { transition: opacity 0s .6s }", ".foo{transition:opacity 0s .6s}");
    test(
      r#"
      .foo {
        transition-property: opacity;
        transition-duration: 0.09s;
        transition-timing-function: ease-in-out;
        transition-delay: 500ms;
      }
    "#,
      indoc! {r#"
      .foo {
        transition: opacity 90ms ease-in-out .5s;
      }
    "#},
    );
    test(
      r#"
      .foo {
        transition: opacity 2s;
        transition-timing-function: ease;
        transition-delay: 500ms;
      }
    "#,
      indoc! {r#"
      .foo {
        transition: opacity 2s .5s;
      }
    "#},
    );
    test(
      r#"
      .foo {
        transition: opacity 500ms;
        transition-timing-function: var(--ease);
      }
    "#,
      indoc! {r#"
      .foo {
        transition: opacity .5s;
        transition-timing-function: var(--ease);
      }
    "#},
    );
    test(
      r#"
      .foo {
        transition-property: opacity;
        transition-duration: 0.09s;
        transition-timing-function: ease-in-out;
        transition-delay: 500ms;
        transition: color 2s;
      }
    "#,
      indoc! {r#"
      .foo {
        transition: color 2s;
      }
    "#},
    );
    test(
      r#"
      .foo {
        transition-property: opacity, color;
        transition-duration: 2s, 4s;
        transition-timing-function: ease-in-out, ease-in;
        transition-delay: 500ms, 0s;
      }
    "#,
      indoc! {r#"
      .foo {
        transition: opacity 2s ease-in-out .5s, color 4s ease-in;
      }
    "#},
    );
    test(
      r#"
      .foo {
        transition-property: opacity, color;
        transition-duration: 2s;
        transition-timing-function: ease-in-out;
        transition-delay: 500ms;
      }
    "#,
      indoc! {r#"
      .foo {
        transition: opacity 2s ease-in-out .5s, color 2s ease-in-out .5s;
      }
    "#},
    );
    test(
      r#"
      .foo {
        transition-property: opacity, color, width, height;
        transition-duration: 2s, 4s;
        transition-timing-function: ease;
        transition-delay: 0s;
      }
    "#,
      indoc! {r#"
      .foo {
        transition: opacity 2s, color 4s, width 2s, height 4s;
      }
    "#},
    );

    test(
      r#"
      .foo {
        -webkit-transition-property: opacity, color;
        -webkit-transition-duration: 2s, 4s;
        -webkit-transition-timing-function: ease-in-out, ease-in;
        -webkit-transition-delay: 500ms, 0s;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-transition: opacity 2s ease-in-out .5s, color 4s ease-in;
      }
    "#},
    );

    test(
      r#"
      .foo {
        -webkit-transition-property: opacity, color;
        -webkit-transition-duration: 2s, 4s;
        -webkit-transition-timing-function: ease-in-out, ease-in;
        -webkit-transition-delay: 500ms, 0s;
        -moz-transition-property: opacity, color;
        -moz-transition-duration: 2s, 4s;
        -moz-transition-timing-function: ease-in-out, ease-in;
        -moz-transition-delay: 500ms, 0s;
        transition-property: opacity, color;
        transition-duration: 2s, 4s;
        transition-timing-function: ease-in-out, ease-in;
        transition-delay: 500ms, 0s;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-transition: opacity 2s ease-in-out .5s, color 4s ease-in;
        -moz-transition: opacity 2s ease-in-out .5s, color 4s ease-in;
        transition: opacity 2s ease-in-out .5s, color 4s ease-in;
      }
    "#},
    );

    test(
      r#"
      .foo {
        -webkit-transition-property: opacity, color;
        -moz-transition-property: opacity, color;
        transition-property: opacity, color;
        -webkit-transition-duration: 2s, 4s;
        -moz-transition-duration: 2s, 4s;
        transition-duration: 2s, 4s;
        -webkit-transition-timing-function: ease-in-out, ease-in;
        transition-timing-function: ease-in-out, ease-in;
        -moz-transition-timing-function: ease-in-out, ease-in;
        -webkit-transition-delay: 500ms, 0s;
        -moz-transition-delay: 500ms, 0s;
        transition-delay: 500ms, 0s;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-transition: opacity 2s ease-in-out .5s, color 4s ease-in;
        -moz-transition: opacity 2s ease-in-out .5s, color 4s ease-in;
        transition: opacity 2s ease-in-out .5s, color 4s ease-in;
      }
    "#},
    );

    test(
      r#"
      .foo {
        -webkit-transition-property: opacity;
        -moz-transition-property: color;
        transition-property: opacity, color;
        -webkit-transition-duration: 2s;
        -moz-transition-duration: 4s;
        transition-duration: 2s, 4s;
        -webkit-transition-timing-function: ease-in-out;
        -moz-transition-timing-function: ease-in-out;
        transition-timing-function: ease-in-out, ease-in;
        -webkit-transition-delay: 500ms;
        -moz-transition-delay: 0s;
        transition-delay: 500ms, 0s;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-transition-property: opacity;
        -moz-transition-property: color;
        transition-property: opacity, color;
        -webkit-transition-duration: 2s;
        -moz-transition-duration: 4s;
        transition-duration: 2s, 4s;
        -webkit-transition-timing-function: ease-in-out;
        -moz-transition-timing-function: ease-in-out;
        -webkit-transition-delay: .5s;
        transition-timing-function: ease-in-out, ease-in;
        -moz-transition-delay: 0s;
        transition-delay: .5s, 0s;
      }
    "#},
    );

    test(
      r#"
      .foo {
        -webkit-transition-property: opacity;
        transition-property: opacity, color;
        -moz-transition-property: color;
        -webkit-transition-duration: 2s;
        transition-duration: 2s, 4s;
        -moz-transition-duration: 4s;
        -webkit-transition-timing-function: ease-in-out;
        transition-timing-function: ease-in-out, ease-in;
        -moz-transition-timing-function: ease-in-out;
        -webkit-transition-delay: 500ms;
        transition-delay: 500ms, 0s;
        -moz-transition-delay: 0s;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-transition-property: opacity;
        transition-property: opacity, color;
        -moz-transition-property: color;
        -webkit-transition-duration: 2s;
        transition-duration: 2s, 4s;
        -moz-transition-duration: 4s;
        -webkit-transition-timing-function: ease-in-out;
        transition-timing-function: ease-in-out, ease-in;
        -webkit-transition-delay: .5s;
        -moz-transition-timing-function: ease-in-out;
        transition-delay: .5s, 0s;
        -moz-transition-delay: 0s;
      }
    "#},
    );

    test(
      r#"
      .foo {
        transition: opacity 2s;
        -webkit-transition-duration: 2s;
      }
    "#,
      indoc! {r#"
      .foo {
        transition: opacity 2s;
        -webkit-transition-duration: 2s;
      }
    "#},
    );

    prefix_test(
      r#"
      .foo {
        transition-property: margin-inline-start;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        transition-property: margin-left;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        transition-property: margin-left;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        transition-property: margin-right;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        transition-property: margin-right;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        transition-property: margin-inline-start, padding-inline-start;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        transition-property: margin-left, padding-left;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        transition-property: margin-left, padding-left;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        transition-property: margin-right, padding-right;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        transition-property: margin-right, padding-right;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        transition-property: margin-inline-start, opacity, padding-inline-start, color;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        transition-property: margin-left, opacity, padding-left, color;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        transition-property: margin-left, opacity, padding-left, color;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        transition-property: margin-right, opacity, padding-right, color;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        transition-property: margin-right, opacity, padding-right, color;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        transition-property: margin-block;
      }
    "#,
      indoc! {r#"
      .foo {
        transition-property: margin-top, margin-bottom;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        transition: margin-inline-start 2s;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        transition: margin-left 2s;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        transition: margin-left 2s;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        transition: margin-right 2s;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        transition: margin-right 2s;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        transition: margin-inline-start 2s, padding-inline-start 2s;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        transition: margin-left 2s, padding-left 2s;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        transition: margin-left 2s, padding-left 2s;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        transition: margin-right 2s, padding-right 2s;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        transition: margin-right 2s, padding-right 2s;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        transition: margin-block-start 2s;
      }
    "#,
      indoc! {r#"
      .foo {
        transition: margin-top 2s;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        transition: transform;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-transition: -webkit-transform, transform;
        transition: -webkit-transform, transform;
      }
    "#
      },
      Browsers {
        safari: Some(6 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        transition: border-start-start-radius;
      }
    "#,
      indoc! {r#"
      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        -webkit-transition: -webkit-border-top-left-radius, border-top-left-radius;
        transition: -webkit-border-top-left-radius, border-top-left-radius;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        -webkit-transition: -webkit-border-top-right-radius, border-top-right-radius;
        transition: -webkit-border-top-right-radius, border-top-right-radius;
      }
    "#
      },
      Browsers {
        safari: Some(4 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        transition: border-start-start-radius;
      }
    "#,
      indoc! {r#"
      .foo:not(:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi)) {
        transition: border-top-left-radius;
      }

      .foo:lang(ae, ar, arc, bcc, bqi, ckb, dv, fa, glk, he, ku, mzn, nqo, pnb, ps, sd, ug, ur, yi) {
        transition: border-top-right-radius;
      }
    "#
      },
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
      .foo {
        -webkit-transition: background 200ms;
        -moz-transition: background 200ms;
        transition: background 230ms;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-transition: background .2s;
        -moz-transition: background .2s;
        transition: background .23s;
      }
    "#},
    );

    prefix_test(
      r#"
      .foo {
        -webkit-transition: background 200ms;
        -moz-transition: background 200ms;
        transition: background 230ms;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-transition: background .2s;
        -moz-transition: background .2s;
        transition: background .23s;
      }
    "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
       .foo {
         transition-property: -webkit-backdrop-filter, backdrop-filter;
       }
       .bar {
         transition-property: backdrop-filter;
       }
       .baz {
         transition-property: -webkit-backdrop-filter;
       }
     "#,
      indoc! {r#"
       .foo, .bar {
         transition-property: -webkit-backdrop-filter, backdrop-filter;
       }

       .baz {
         transition-property: -webkit-backdrop-filter;
       }
     "#
      },
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
       .foo {
         transition-property: -webkit-border-radius, -webkit-border-radius, -moz-border-radius;
       }
     "#,
      indoc! {r#"
       .foo {
         transition-property: -webkit-border-radius, -moz-border-radius;
       }
     "#
      },
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
       .foo {
         transition: -webkit-backdrop-filter, backdrop-filter;
       }
       .bar {
         transition: backdrop-filter;
       }
       .baz {
         transition: -webkit-backdrop-filter;
       }
     "#,
      indoc! {r#"
       .foo, .bar {
         transition: -webkit-backdrop-filter, backdrop-filter;
       }

       .baz {
         transition: -webkit-backdrop-filter;
       }
     "#
      },
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_animation() {
    minify_test(".foo { animation-name: test }", ".foo{animation-name:test}");
    minify_test(".foo { animation-name: \"test\" }", ".foo{animation-name:test}");
    minify_test(".foo { animation-name: foo, bar }", ".foo{animation-name:foo,bar}");
    minify_test(".foo { animation-name: \"none\" }", ".foo{animation-name:\"none\"}");
    minify_test(
      ".foo { animation-name: \"none\", foo }",
      ".foo{animation-name:\"none\",foo}",
    );
    let name = crate::properties::animation::AnimationName::parse_string("default");
    assert!(matches!(name, Err(..)));

    minify_test(".foo { animation-name: none }", ".foo{animation-name:none}");
    minify_test(".foo { animation-name: none, none }", ".foo{animation-name:none,none}");

    // Test CSS-wide keywords
    minify_test(".foo { animation-name: unset }", ".foo{animation-name:unset}");
    minify_test(".foo { animation-name: \"unset\" }", ".foo{animation-name:\"unset\"}");
    minify_test(".foo { animation-name: \"revert\" }", ".foo{animation-name:\"revert\"}");
    minify_test(
      ".foo { animation-name: \"unset\", \"revert\"}",
      ".foo{animation-name:\"unset\",\"revert\"}",
    );
    minify_test(
      ".foo { animation-name: foo, \"revert\"}",
      ".foo{animation-name:foo,\"revert\"}",
    );
    minify_test(
      ".foo { animation-name: \"string\", \"revert\"}",
      ".foo{animation-name:string,\"revert\"}",
    );
    minify_test(
      ".foo { animation-name: \"string\", foo, \"revert\"}",
      ".foo{animation-name:string,foo,\"revert\"}",
    );
    minify_test(
      ".foo { animation-name: \"default\" }",
      ".foo{animation-name:\"default\"}",
    );
    minify_test(".foo { animation-duration: 100ms }", ".foo{animation-duration:.1s}");
    minify_test(
      ".foo { animation-duration: 100ms, 2000ms }",
      ".foo{animation-duration:.1s,2s}",
    );
    minify_test(
      ".foo { animation-timing-function: ease }",
      ".foo{animation-timing-function:ease}",
    );
    minify_test(
      ".foo { animation-timing-function: cubic-bezier(0.42, 0, 1, 1) }",
      ".foo{animation-timing-function:ease-in}",
    );
    minify_test(
      ".foo { animation-timing-function: ease, cubic-bezier(0.42, 0, 1, 1) }",
      ".foo{animation-timing-function:ease,ease-in}",
    );
    minify_test(
      ".foo { animation-iteration-count: 5 }",
      ".foo{animation-iteration-count:5}",
    );
    minify_test(
      ".foo { animation-iteration-count: 2.5 }",
      ".foo{animation-iteration-count:2.5}",
    );
    minify_test(
      ".foo { animation-iteration-count: 2.0 }",
      ".foo{animation-iteration-count:2}",
    );
    minify_test(
      ".foo { animation-iteration-count: infinite }",
      ".foo{animation-iteration-count:infinite}",
    );
    minify_test(
      ".foo { animation-iteration-count: 1, infinite }",
      ".foo{animation-iteration-count:1,infinite}",
    );
    minify_test(
      ".foo { animation-direction: reverse }",
      ".foo{animation-direction:reverse}",
    );
    minify_test(
      ".foo { animation-direction: alternate, reverse }",
      ".foo{animation-direction:alternate,reverse}",
    );
    minify_test(
      ".foo { animation-play-state: paused }",
      ".foo{animation-play-state:paused}",
    );
    minify_test(
      ".foo { animation-play-state: running, paused }",
      ".foo{animation-play-state:running,paused}",
    );
    minify_test(".foo { animation-delay: 100ms }", ".foo{animation-delay:.1s}");
    minify_test(
      ".foo { animation-delay: 100ms, 2000ms }",
      ".foo{animation-delay:.1s,2s}",
    );
    minify_test(
      ".foo { animation-fill-mode: forwards }",
      ".foo{animation-fill-mode:forwards}",
    );
    minify_test(
      ".foo { animation-fill-mode: Backwards,forwards }",
      ".foo{animation-fill-mode:backwards,forwards}",
    );
    minify_test(".foo { animation: none }", ".foo{animation:none}");
    minify_test(".foo { animation: \"none\" }", ".foo{animation:\"none\"}");
    minify_test(".foo { animation: \"None\" }", ".foo{animation:\"None\"}");
    minify_test(".foo { animation: \"none\", none }", ".foo{animation:\"none\",none}");
    minify_test(".foo { animation: none, none }", ".foo{animation:none,none}");
    minify_test(".foo { animation: \"none\" none }", ".foo{animation:\"none\"}");
    minify_test(".foo { animation: none none }", ".foo{animation:none}");

    // Test animation-name + animation-fill-mode
    minify_test(
      ".foo { animation: 2s both \"none\"}",
      ".foo{animation:2s both \"none\"}",
    );
    minify_test(
      ".foo { animation: both \"none\" 2s}",
      ".foo{animation:2s both \"none\"}",
    );
    minify_test(".foo { animation: \"none\" 2s none}", ".foo{animation:2s \"none\"}");
    minify_test(".foo { animation: none \"none\" 2s}", ".foo{animation:2s \"none\"}");
    minify_test(
      ".foo { animation: none, \"none\" 2s forwards}",
      ".foo{animation:none,2s forwards \"none\"}",
    );

    minify_test(".foo { animation: \"unset\" }", ".foo{animation:\"unset\"}");
    minify_test(".foo { animation: \"string\" .5s }", ".foo{animation:.5s string}");
    minify_test(".foo { animation: \"unset\" .5s }", ".foo{animation:.5s \"unset\"}");
    minify_test(
      ".foo { animation: none, \"unset\" .5s}",
      ".foo{animation:none,.5s \"unset\"}",
    );
    minify_test(
      ".foo { animation: \"unset\" 0s 3s infinite, none }",
      ".foo{animation:0s 3s infinite \"unset\",none}",
    );

    minify_test(".foo { animation: \"infinite\" 2s 1 }", ".foo{animation:2s 1 infinite}");
    minify_test(".foo { animation: \"paused\" 2s }", ".foo{animation:2s running paused}");
    minify_test(
      ".foo { animation: \"forwards\" 2s }",
      ".foo{animation:2s none forwards}",
    );
    minify_test(
      ".foo { animation: \"reverse\" 2s }",
      ".foo{animation:2s normal reverse}",
    );
    minify_test(
      ".foo { animation: \"reverse\" 2s alternate }",
      ".foo{animation:2s alternate reverse}",
    );

    minify_test(
      ".foo { animation: 3s ease-in 1s infinite reverse both running slidein }",
      ".foo{animation:3s ease-in 1s infinite reverse both slidein}",
    );
    minify_test(
      ".foo { animation: 3s slidein paused ease 1s 1 reverse both }",
      ".foo{animation:3s 1s reverse both paused slidein}",
    );
    minify_test(".foo { animation: 3s ease ease }", ".foo{animation:3s ease ease}");
    minify_test(
      ".foo { animation: 3s cubic-bezier(0.25, 0.1, 0.25, 1) foo }",
      ".foo{animation:3s foo}",
    );
    minify_test(
      ".foo { animation: foo 0s 3s infinite }",
      ".foo{animation:0s 3s infinite foo}",
    );
    minify_test(".foo { animation: foo 3s --test }", ".foo{animation:3s foo --test}");
    minify_test(".foo { animation: foo 3s scroll() }", ".foo{animation:3s foo scroll()}");
    minify_test(
      ".foo { animation: foo 3s scroll(block) }",
      ".foo{animation:3s foo scroll()}",
    );
    minify_test(
      ".foo { animation: foo 3s scroll(root inline) }",
      ".foo{animation:3s foo scroll(root inline)}",
    );
    minify_test(
      ".foo { animation: foo 3s scroll(inline root) }",
      ".foo{animation:3s foo scroll(root inline)}",
    );
    minify_test(
      ".foo { animation: foo 3s scroll(inline nearest) }",
      ".foo{animation:3s foo scroll(inline)}",
    );
    minify_test(
      ".foo { animation: foo 3s view(block) }",
      ".foo{animation:3s foo view()}",
    );
    minify_test(
      ".foo { animation: foo 3s view(inline) }",
      ".foo{animation:3s foo view(inline)}",
    );
    minify_test(
      ".foo { animation: foo 3s view(inline 10px 10px) }",
      ".foo{animation:3s foo view(inline 10px)}",
    );
    minify_test(
      ".foo { animation: foo 3s view(inline 10px 12px) }",
      ".foo{animation:3s foo view(inline 10px 12px)}",
    );
    minify_test(
      ".foo { animation: foo 3s view(inline auto auto) }",
      ".foo{animation:3s foo view(inline)}",
    );
    minify_test(".foo { animation: foo 3s auto }", ".foo{animation:3s foo}");
    minify_test(".foo { animation-composition: add }", ".foo{animation-composition:add}");
    test(
      r#"
      .foo {
        animation-name: foo;
        animation-duration: 0.09s;
        animation-timing-function: ease-in-out;
        animation-iteration-count: 2;
        animation-direction: alternate;
        animation-play-state: running;
        animation-delay: 100ms;
        animation-fill-mode: forwards;
        animation-timeline: auto;
      }
    "#,
      indoc! {r#"
      .foo {
        animation: 90ms ease-in-out .1s 2 alternate forwards foo;
      }
    "#},
    );
    test(
      r#"
      .foo {
        animation-name: foo, bar;
        animation-duration: 0.09s, 200ms;
        animation-timing-function: ease-in-out, ease;
        animation-iteration-count: 2, 1;
        animation-direction: alternate, normal;
        animation-play-state: running, paused;
        animation-delay: 100ms, 0s;
        animation-fill-mode: forwards, none;
        animation-timeline: auto, auto;
      }
    "#,
      indoc! {r#"
      .foo {
        animation: 90ms ease-in-out .1s 2 alternate forwards foo, .2s paused bar;
      }
    "#},
    );
    test(
      r#"
      .foo {
        animation: bar 200ms;
        animation-timing-function: ease-in-out;
      }
    "#,
      indoc! {r#"
      .foo {
        animation: .2s ease-in-out bar;
      }
    "#},
    );
    test(
      r#"
      .foo {
        animation: bar 200ms;
        animation-timing-function: var(--ease);
      }
    "#,
      indoc! {r#"
      .foo {
        animation: .2s bar;
        animation-timing-function: var(--ease);
      }
    "#},
    );
    test(
      r#"
      .foo {
        animation-name: foo, bar;
        animation-duration: 0.09s;
        animation-timing-function: ease-in-out;
        animation-iteration-count: 2;
        animation-direction: alternate;
        animation-play-state: running;
        animation-delay: 100ms;
        animation-fill-mode: forwards;
        animation-timeline: auto;
      }
    "#,
      indoc! {r#"
      .foo {
        animation-name: foo, bar;
        animation-duration: 90ms;
        animation-timing-function: ease-in-out;
        animation-iteration-count: 2;
        animation-direction: alternate;
        animation-play-state: running;
        animation-delay: .1s;
        animation-fill-mode: forwards;
        animation-timeline: auto;
      }
    "#},
    );
    test(
      r#"
      .foo {
        animation-name: foo;
        animation-duration: 0.09s;
        animation-timing-function: ease-in-out;
        animation-iteration-count: 2;
        animation-direction: alternate;
        animation-play-state: running;
        animation-delay: 100ms;
        animation-fill-mode: forwards;
        animation-timeline: scroll();
      }
    "#,
      indoc! {r#"
      .foo {
        animation: 90ms ease-in-out .1s 2 alternate forwards foo scroll();
      }
    "#},
    );
    test(
      r#"
      .foo {
        animation-name: foo;
        animation-duration: 0.09s;
        animation-timing-function: ease-in-out;
        animation-iteration-count: 2;
        animation-direction: alternate;
        animation-play-state: running;
        animation-delay: 100ms;
        animation-fill-mode: forwards;
        animation-timeline: scroll(), view();
      }
    "#,
      indoc! {r#"
      .foo {
        animation-name: foo;
        animation-duration: 90ms;
        animation-timing-function: ease-in-out;
        animation-iteration-count: 2;
        animation-direction: alternate;
        animation-play-state: running;
        animation-delay: .1s;
        animation-fill-mode: forwards;
        animation-timeline: scroll(), view();
      }
    "#},
    );
    test(
      r#"
      .foo {
        -webkit-animation-name: foo;
        -webkit-animation-duration: 0.09s;
        -webkit-animation-timing-function: ease-in-out;
        -webkit-animation-iteration-count: 2;
        -webkit-animation-direction: alternate;
        -webkit-animation-play-state: running;
        -webkit-animation-delay: 100ms;
        -webkit-animation-fill-mode: forwards;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-animation: 90ms ease-in-out .1s 2 alternate forwards foo;
      }
    "#},
    );
    test(
      r#"
      .foo {
        -moz-animation: bar 200ms;
        -moz-animation-timing-function: ease-in-out;
      }
    "#,
      indoc! {r#"
      .foo {
        -moz-animation: .2s ease-in-out bar;
      }
    "#},
    );
    test(
      r#"
      .foo {
        -webkit-animation: bar 200ms;
        -webkit-animation-timing-function: ease-in-out;
        -moz-animation: bar 200ms;
        -moz-animation-timing-function: ease-in-out;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-animation: .2s ease-in-out bar;
        -moz-animation: .2s ease-in-out bar;
      }
    "#},
    );

    prefix_test(
      r#"
      .foo {
        animation: .2s ease-in-out bar;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-animation: .2s ease-in-out bar;
        -moz-animation: .2s ease-in-out bar;
        animation: .2s ease-in-out bar;
      }
    "#},
      Browsers {
        firefox: Some(6 << 16),
        safari: Some(6 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-animation: .2s ease-in-out bar;
        -moz-animation: .2s ease-in-out bar;
        animation: .2s ease-in-out bar;
      }
    "#,
      indoc! {r#"
      .foo {
        animation: .2s ease-in-out bar;
      }
    "#},
      Browsers {
        firefox: Some(20 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        animation: 200ms var(--ease) bar;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-animation: .2s var(--ease) bar;
        -moz-animation: .2s var(--ease) bar;
        animation: .2s var(--ease) bar;
      }
    "#},
      Browsers {
        firefox: Some(6 << 16),
        safari: Some(6 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        animation: .2s ease-in-out bar scroll();
      }
    "#,
      indoc! {r#"
      .foo {
        animation: .2s ease-in-out bar;
        animation-timeline: scroll();
      }
    "#},
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        animation: .2s ease-in-out bar scroll();
      }
    "#,
      indoc! {r#"
      .foo {
        animation: .2s ease-in-out bar scroll();
      }
    "#},
      Browsers {
        chrome: Some(120 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        animation: .2s ease-in-out bar scroll();
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-animation: .2s ease-in-out bar;
        animation: .2s ease-in-out bar;
        animation-timeline: scroll();
      }
    "#},
      Browsers {
        safari: Some(6 << 16),
        ..Browsers::default()
      },
    );

    minify_test(
      ".foo { animation-range-start: entry 10% }",
      ".foo{animation-range-start:entry 10%}",
    );
    minify_test(
      ".foo { animation-range-start: entry 0% }",
      ".foo{animation-range-start:entry}",
    );
    minify_test(
      ".foo { animation-range-start: entry }",
      ".foo{animation-range-start:entry}",
    );
    minify_test(".foo { animation-range-start: 50% }", ".foo{animation-range-start:50%}");
    minify_test(
      ".foo { animation-range-end: exit 10% }",
      ".foo{animation-range-end:exit 10%}",
    );
    minify_test(
      ".foo { animation-range-end: exit 100% }",
      ".foo{animation-range-end:exit}",
    );
    minify_test(".foo { animation-range-end: exit }", ".foo{animation-range-end:exit}");
    minify_test(".foo { animation-range-end: 50% }", ".foo{animation-range-end:50%}");
    minify_test(
      ".foo { animation-range: entry 10% exit 90% }",
      ".foo{animation-range:entry 10% exit 90%}",
    );
    minify_test(
      ".foo { animation-range: entry 0% exit 100% }",
      ".foo{animation-range:entry exit}",
    );
    minify_test(".foo { animation-range: entry }", ".foo{animation-range:entry}");
    minify_test(
      ".foo { animation-range: entry 0% entry 100% }",
      ".foo{animation-range:entry}",
    );
    minify_test(".foo { animation-range: 50% normal }", ".foo{animation-range:50%}");
    minify_test(
      ".foo { animation-range: normal normal }",
      ".foo{animation-range:normal}",
    );
    test(
      r#"
      .foo {
        animation-range-start: entry 10%;
        animation-range-end: exit 90%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: entry 10% exit 90%;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range-start: entry 0%;
        animation-range-end: entry 100%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: entry;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range-start: entry 0%;
        animation-range-end: exit 100%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: entry exit;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range-start: 10%;
        animation-range-end: normal;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: 10%;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range-start: 10%;
        animation-range-end: 90%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: 10% 90%;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range-start: entry 10%;
        animation-range-end: exit 100%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: entry 10% exit;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range-start: 10%;
        animation-range-end: exit 90%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: 10% exit 90%;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range-start: entry 10%;
        animation-range-end: 90%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: entry 10% 90%;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range: entry;
        animation-range-end: 90%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: entry 90%;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range: entry;
        animation-range-end: var(--end);
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: entry;
        animation-range-end: var(--end);
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range-start: entry 10%, entry 50%;
        animation-range-end: exit 90%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range-start: entry 10%, entry 50%;
        animation-range-end: exit 90%;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range-start: entry 10%, entry 50%;
        animation-range-end: exit 90%, exit 100%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation-range: entry 10% exit 90%, entry 50% exit;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range: entry;
        animation-range-end: 90%;
        animation: spin 100ms;
      }
      "#,
      indoc! {r#"
      .foo {
        animation: .1s spin;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation: spin 100ms;
        animation-range: entry;
        animation-range-end: 90%;
      }
      "#,
      indoc! {r#"
      .foo {
        animation: .1s spin;
        animation-range: entry 90%;
      }
      "#},
    );
    test(
      r#"
      .foo {
        animation-range: entry;
        animation-range-end: 90%;
        animation: var(--animation) 100ms;
      }
      "#,
      indoc! {r#"
      .foo {
        animation: var(--animation) .1s;
      }
      "#},
    );
  }

  #[test]
  fn test_transform() {
    minify_test(
      ".foo { transform: translate(2px, 3px)",
      ".foo{transform:translate(2px,3px)}",
    );
    minify_test(
      ".foo { transform: translate(2px, 0px)",
      ".foo{transform:translate(2px)}",
    );
    minify_test(
      ".foo { transform: translate(0px, 2px)",
      ".foo{transform:translateY(2px)}",
    );
    minify_test(".foo { transform: translateX(2px)", ".foo{transform:translate(2px)}");
    minify_test(".foo { transform: translateY(2px)", ".foo{transform:translateY(2px)}");
    minify_test(".foo { transform: translateZ(2px)", ".foo{transform:translateZ(2px)}");
    minify_test(
      ".foo { transform: translate3d(2px, 3px, 4px)",
      ".foo{transform:translate3d(2px,3px,4px)}",
    );
    minify_test(
      ".foo { transform: translate3d(10%, 20%, 4px)",
      ".foo{transform:translate3d(10%,20%,4px)}",
    );
    minify_test(
      ".foo { transform: translate3d(2px, 0px, 0px)",
      ".foo{transform:translate(2px)}",
    );
    minify_test(
      ".foo { transform: translate3d(0px, 2px, 0px)",
      ".foo{transform:translateY(2px)}",
    );
    minify_test(
      ".foo { transform: translate3d(0px, 0px, 2px)",
      ".foo{transform:translateZ(2px)}",
    );
    minify_test(
      ".foo { transform: translate3d(2px, 3px, 0px)",
      ".foo{transform:translate(2px,3px)}",
    );
    minify_test(".foo { transform: scale(2, 3)", ".foo{transform:scale(2,3)}");
    minify_test(".foo { transform: scale(10%, 20%)", ".foo{transform:scale(.1,.2)}");
    minify_test(".foo { transform: scale(2, 2)", ".foo{transform:scale(2)}");
    minify_test(".foo { transform: scale(2, 1)", ".foo{transform:scaleX(2)}");
    minify_test(".foo { transform: scale(1, 2)", ".foo{transform:scaleY(2)}");
    minify_test(".foo { transform: scaleX(2)", ".foo{transform:scaleX(2)}");
    minify_test(".foo { transform: scaleY(2)", ".foo{transform:scaleY(2)}");
    minify_test(".foo { transform: scaleZ(2)", ".foo{transform:scaleZ(2)}");
    minify_test(".foo { transform: scale3d(2, 3, 4)", ".foo{transform:scale3d(2,3,4)}");
    minify_test(".foo { transform: scale3d(2, 1, 1)", ".foo{transform:scaleX(2)}");
    minify_test(".foo { transform: scale3d(1, 2, 1)", ".foo{transform:scaleY(2)}");
    minify_test(".foo { transform: scale3d(1, 1, 2)", ".foo{transform:scaleZ(2)}");
    minify_test(".foo { transform: scale3d(2, 2, 1)", ".foo{transform:scale(2)}");
    minify_test(".foo { transform: rotate(20deg)", ".foo{transform:rotate(20deg)}");
    minify_test(".foo { transform: rotateX(20deg)", ".foo{transform:rotateX(20deg)}");
    minify_test(".foo { transform: rotateY(20deg)", ".foo{transform:rotateY(20deg)}");
    minify_test(".foo { transform: rotateZ(20deg)", ".foo{transform:rotate(20deg)}");
    minify_test(".foo { transform: rotate(360deg)", ".foo{transform:rotate(360deg)}");
    minify_test(
      ".foo { transform: rotate3d(2, 3, 4, 20deg)",
      ".foo{transform:rotate3d(2,3,4,20deg)}",
    );
    minify_test(
      ".foo { transform: rotate3d(1, 0, 0, 20deg)",
      ".foo{transform:rotateX(20deg)}",
    );
    minify_test(
      ".foo { transform: rotate3d(0, 1, 0, 20deg)",
      ".foo{transform:rotateY(20deg)}",
    );
    minify_test(
      ".foo { transform: rotate3d(0, 0, 1, 20deg)",
      ".foo{transform:rotate(20deg)}",
    );
    minify_test(".foo { transform: rotate(405deg)}", ".foo{transform:rotate(405deg)}");
    minify_test(".foo { transform: rotateX(405deg)}", ".foo{transform:rotateX(405deg)}");
    minify_test(".foo { transform: rotateY(405deg)}", ".foo{transform:rotateY(405deg)}");
    minify_test(".foo { transform: rotate(-200deg)}", ".foo{transform:rotate(-200deg)}");
    minify_test(".foo { transform: rotate(0)", ".foo{transform:rotate(0)}");
    minify_test(".foo { transform: rotate(0deg)", ".foo{transform:rotate(0)}");
    minify_test(
      ".foo { transform: rotateX(-200deg)}",
      ".foo{transform:rotateX(-200deg)}",
    );
    minify_test(
      ".foo { transform: rotateY(-200deg)}",
      ".foo{transform:rotateY(-200deg)}",
    );
    minify_test(
      ".foo { transform: rotate3d(1, 1, 0, -200deg)",
      ".foo{transform:rotate3d(1,1,0,-200deg)}",
    );
    minify_test(".foo { transform: skew(20deg)", ".foo{transform:skew(20deg)}");
    minify_test(".foo { transform: skew(20deg, 0deg)", ".foo{transform:skew(20deg)}");
    minify_test(".foo { transform: skew(0deg, 20deg)", ".foo{transform:skewY(20deg)}");
    minify_test(".foo { transform: skewX(20deg)", ".foo{transform:skew(20deg)}");
    minify_test(".foo { transform: skewY(20deg)", ".foo{transform:skewY(20deg)}");
    minify_test(
      ".foo { transform: perspective(10px)",
      ".foo{transform:perspective(10px)}",
    );
    minify_test(
      ".foo { transform: matrix(1, 2, -1, 1, 80, 80)",
      ".foo{transform:matrix(1,2,-1,1,80,80)}",
    );
    minify_test(
      ".foo { transform: matrix3d(1, 0, 0, 0, 0, 1, 6, 0, 0, 0, 1, 0, 50, 100, 0, 1.1)",
      ".foo{transform:matrix3d(1,0,0,0,0,1,6,0,0,0,1,0,50,100,0,1.1)}",
    );
    // TODO: Re-enable with a better solution
    //       See: https://github.com/parcel-bundler/lightningcss/issues/288
    // minify_test(
    //   ".foo{transform:translate(100px,200px) rotate(45deg) skew(10deg) scale(2)}",
    //   ".foo{transform:matrix(1.41421,1.41421,-1.16485,1.66358,100,200)}",
    // );
    // minify_test(
    //   ".foo{transform:translate(200px,300px) translate(100px,200px) scale(2)}",
    //   ".foo{transform:matrix(2,0,0,2,300,500)}",
    // );
    minify_test(
      ".foo{transform:translate(100px,200px) rotate(45deg)}",
      ".foo{transform:translate(100px,200px)rotate(45deg)}",
    );
    minify_test(
      ".foo{transform:rotate3d(1, 1, 1, 45deg) translate3d(100px, 100px, 10px)}",
      ".foo{transform:rotate3d(1,1,1,45deg)translate3d(100px,100px,10px)}",
    );
    // TODO: Re-enable with a better solution
    //       See: https://github.com/parcel-bundler/lightningcss/issues/288
    // minify_test(
    //   ".foo{transform:translate3d(100px, 100px, 10px) skew(10deg) scale3d(2, 3, 4)}",
    //   ".foo{transform:matrix3d(2,0,0,0,.528981,3,0,0,0,0,4,0,100,100,10,1)}",
    // );
    // minify_test(
    //   ".foo{transform:matrix3d(0.804737854124365, 0.5058793634016805, -0.31061721752604554, 0, -0.31061721752604554, 0.804737854124365, 0.5058793634016805, 0, 0.5058793634016805, -0.31061721752604554, 0.804737854124365, 0, 100, 100, 10, 1)}",
    //   ".foo{transform:translate3d(100px,100px,10px)rotate3d(1,1,1,45deg)}"
    // );
    // minify_test(
    //   ".foo{transform:matrix3d(1, 0, 0, 0, 0, 0.7071067811865476, 0.7071067811865475, 0, 0, -0.7071067811865475, 0.7071067811865476, 0, 100, 100, 10, 1)}",
    //   ".foo{transform:translate3d(100px,100px,10px)rotateX(45deg)}"
    // );
    // minify_test(
    //   ".foo{transform:translate3d(100px, 200px, 10px) translate(100px, 100px)}",
    //   ".foo{transform:translate3d(200px,300px,10px)}",
    // );
    // minify_test(
    //   ".foo{transform:rotate(45deg) rotate(45deg)}",
    //   ".foo{transform:rotate(90deg)}",
    // );
    // minify_test(
    //   ".foo{transform:matrix(0.7071067811865476, 0.7071067811865475, -0.7071067811865475, 0.7071067811865476, 100, 100)}",
    //   ".foo{transform:translate(100px,100px)rotate(45deg)}"
    // );
    // minify_test(
    //   ".foo{transform:translateX(2in) translateX(50px)}",
    //   ".foo{transform:translate(242px)}",
    // );
    minify_test(
      ".foo{transform:translateX(calc(2in + 50px))}",
      ".foo{transform:translate(242px)}",
    );
    minify_test(".foo{transform:translateX(50%)}", ".foo{transform:translate(50%)}");
    minify_test(
      ".foo{transform:translateX(calc(50% - 100px + 20px))}",
      ".foo{transform:translate(calc(50% - 80px))}",
    );
    minify_test(
      ".foo{transform:rotate(calc(10deg + 20deg))}",
      ".foo{transform:rotate(30deg)}",
    );
    minify_test(
      ".foo{transform:rotate(calc(10deg + 0.349066rad))}",
      ".foo{transform:rotate(30deg)}",
    );
    minify_test(
      ".foo{transform:rotate(calc(10deg + 1.5turn))}",
      ".foo{transform:rotate(550deg)}",
    );
    minify_test(
      ".foo{transform:rotate(calc(10deg * 2))}",
      ".foo{transform:rotate(20deg)}",
    );
    minify_test(
      ".foo{transform:rotate(calc(-10deg * 2))}",
      ".foo{transform:rotate(-20deg)}",
    );
    minify_test(
      ".foo{transform:rotate(calc(10deg + var(--test)))}",
      ".foo{transform:rotate(calc(10deg + var(--test)))}",
    );
    minify_test(".foo { transform: scale(calc(10% + 20%))", ".foo{transform:scale(.3)}");
    minify_test(".foo { transform: scale(calc(.1 + .2))", ".foo{transform:scale(.3)}");

    minify_test(
      ".foo { -webkit-transform: scale(calc(10% + 20%))",
      ".foo{-webkit-transform:scale(.3)}",
    );

    minify_test(".foo { translate: 1px 2px 3px }", ".foo{translate:1px 2px 3px}");
    minify_test(".foo { translate: 1px 0px 0px }", ".foo{translate:1px}");
    minify_test(".foo { translate: 1px 2px 0px }", ".foo{translate:1px 2px}");
    minify_test(".foo { translate: 1px 0px 2px }", ".foo{translate:1px 0 2px}");
    minify_test(".foo { translate: none }", ".foo{translate:none}");
    minify_test(".foo { rotate: 10deg }", ".foo{rotate:10deg}");
    minify_test(".foo { rotate: z 10deg }", ".foo{rotate:10deg}");
    minify_test(".foo { rotate: 0 0 1 10deg }", ".foo{rotate:10deg}");
    minify_test(".foo { rotate: x 10deg }", ".foo{rotate:x 10deg}");
    minify_test(".foo { rotate: 1 0 0 10deg }", ".foo{rotate:x 10deg}");
    minify_test(".foo { rotate: y 10deg }", ".foo{rotate:y 10deg}");
    minify_test(".foo { rotate: 0 1 0 10deg }", ".foo{rotate:y 10deg}");
    minify_test(".foo { rotate: 1 1 1 10deg }", ".foo{rotate:1 1 1 10deg}");
    minify_test(".foo { rotate: 0 0 1 0deg }", ".foo{rotate:none}");
    minify_test(".foo { rotate: none }", ".foo{rotate:none}");
    minify_test(".foo { scale: 1 }", ".foo{scale:1}");
    minify_test(".foo { scale: 1 1 }", ".foo{scale:1}");
    minify_test(".foo { scale: 1 1 1 }", ".foo{scale:1}");
    minify_test(".foo { scale: none }", ".foo{scale:none}");
    minify_test(".foo { scale: 1 0 }", ".foo{scale:1 0}");
    minify_test(".foo { scale: 1 0 1 }", ".foo{scale:1 0}");
    minify_test(".foo { scale: 1 0 0 }", ".foo{scale:1 0 0}");

    // TODO: Re-enable with a better solution
    //       See: https://github.com/parcel-bundler/lightningcss/issues/288
    // minify_test(".foo { transform: scale(3); scale: 0.5 }", ".foo{transform:scale(1.5)}");
    minify_test(".foo { scale: 0.5; transform: scale(3); }", ".foo{transform:scale(3)}");

    prefix_test(
      r#"
      .foo {
        transform: scale(0.5);
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-transform: scale(.5);
        -moz-transform: scale(.5);
        transform: scale(.5);
      }
    "#},
      Browsers {
        firefox: Some(6 << 16),
        safari: Some(6 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        transform: var(--transform);
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-transform: var(--transform);
        -moz-transform: var(--transform);
        transform: var(--transform);
      }
    "#},
      Browsers {
        firefox: Some(6 << 16),
        safari: Some(6 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
      .foo {
        transform: translateX(-50%);
        transform: translateX(20px);
      }
      "#,
      indoc! {r#"
      .foo {
        transform: translateX(20px);
      }
      "#},
    );
  }

  #[test]
  pub fn test_gradients() {
    minify_test(
      ".foo { background: linear-gradient(yellow, blue) }",
      ".foo{background:linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(to bottom, yellow, blue); }",
      ".foo{background:linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(180deg, yellow, blue); }",
      ".foo{background:linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(0.5turn, yellow, blue); }",
      ".foo{background:linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(yellow 10%, blue 20%) }",
      ".foo{background:linear-gradient(#ff0 10%,#00f 20%)}",
    );
    minify_test(
      ".foo { background: linear-gradient(to top, blue, yellow); }",
      ".foo{background:linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(to top, blue 10%, yellow 20%); }",
      ".foo{background:linear-gradient(#ff0 80%,#00f 90%)}",
    );
    minify_test(
      ".foo { background: linear-gradient(to top, blue 10px, yellow 20px); }",
      ".foo{background:linear-gradient(0deg,#00f 10px,#ff0 20px)}",
    );
    minify_test(
      ".foo { background: linear-gradient(135deg, yellow, blue); }",
      ".foo{background:linear-gradient(135deg,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(yellow, blue 20%, #0f0); }",
      ".foo{background:linear-gradient(#ff0,#00f 20%,#0f0)}",
    );
    minify_test(
      ".foo { background: linear-gradient(to top right, red, white, blue) }",
      ".foo{background:linear-gradient(to top right,red,#fff,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(yellow, blue calc(10% * 2), #0f0); }",
      ".foo{background:linear-gradient(#ff0,#00f 20%,#0f0)}",
    );
    minify_test(
      ".foo { background: linear-gradient(yellow, 20%, blue); }",
      ".foo{background:linear-gradient(#ff0,20%,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(yellow, 50%, blue); }",
      ".foo{background:linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(yellow, 20px, blue); }",
      ".foo{background:linear-gradient(#ff0,20px,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(yellow, 50px, blue); }",
      ".foo{background:linear-gradient(#ff0,50px,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(yellow, 50px, blue); }",
      ".foo{background:linear-gradient(#ff0,50px,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(yellow, red 30% 40%, blue); }",
      ".foo{background:linear-gradient(#ff0,red 30% 40%,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(yellow, red 30%, red 40%, blue); }",
      ".foo{background:linear-gradient(#ff0,red 30% 40%,#00f)}",
    );
    minify_test(
      ".foo { background: linear-gradient(0, yellow, blue); }",
      ".foo{background:linear-gradient(#00f,#ff0)}",
    );
    minify_test(
      ".foo { background: -webkit-linear-gradient(yellow, blue) }",
      ".foo{background:-webkit-linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -webkit-linear-gradient(bottom, yellow, blue); }",
      ".foo{background:-webkit-linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -webkit-linear-gradient(top right, red, white, blue) }",
      ".foo{background:-webkit-linear-gradient(top right,red,#fff,#00f)}",
    );
    minify_test(
      ".foo { background: -moz-linear-gradient(yellow, blue) }",
      ".foo{background:-moz-linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -moz-linear-gradient(bottom, yellow, blue); }",
      ".foo{background:-moz-linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -moz-linear-gradient(top right, red, white, blue) }",
      ".foo{background:-moz-linear-gradient(top right,red,#fff,#00f)}",
    );
    minify_test(
      ".foo { background: -o-linear-gradient(yellow, blue) }",
      ".foo{background:-o-linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -o-linear-gradient(bottom, yellow, blue); }",
      ".foo{background:-o-linear-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -o-linear-gradient(top right, red, white, blue) }",
      ".foo{background:-o-linear-gradient(top right,red,#fff,#00f)}",
    );
    minify_test(
      ".foo { background: -webkit-gradient(linear, left top, left bottom, from(blue), to(yellow)) }",
      ".foo{background:-webkit-gradient(linear,0 0,0 100%,from(#00f),to(#ff0))}",
    );
    minify_test(
      ".foo { background: -webkit-gradient(linear, left top, left bottom, from(blue), color-stop(50%, red), to(yellow)) }",
      ".foo{background:-webkit-gradient(linear,0 0,0 100%,from(#00f),color-stop(.5,red),to(#ff0))}"
    );
    minify_test(
      ".foo { background: -webkit-gradient(linear, left top, left bottom, color-stop(0%, blue), color-stop(50%, red), color-stop(100%, yellow)) }",
      ".foo{background:-webkit-gradient(linear,0 0,0 100%,from(#00f),color-stop(.5,red),to(#ff0))}"
    );
    minify_test(
      ".foo { background: repeating-linear-gradient(yellow 10px, blue 50px) }",
      ".foo{background:repeating-linear-gradient(#ff0 10px,#00f 50px)}",
    );
    minify_test(
      ".foo { background: -webkit-repeating-linear-gradient(yellow 10px, blue 50px) }",
      ".foo{background:-webkit-repeating-linear-gradient(#ff0 10px,#00f 50px)}",
    );
    minify_test(
      ".foo { background: -moz-repeating-linear-gradient(yellow 10px, blue 50px) }",
      ".foo{background:-moz-repeating-linear-gradient(#ff0 10px,#00f 50px)}",
    );
    minify_test(
      ".foo { background: -o-repeating-linear-gradient(yellow 10px, blue 50px) }",
      ".foo{background:-o-repeating-linear-gradient(#ff0 10px,#00f 50px)}",
    );
    minify_test(
      ".foo { background: radial-gradient(yellow, blue) }",
      ".foo{background:radial-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(at top left, yellow, blue) }",
      ".foo{background:radial-gradient(at 0 0,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(5em circle at top left, yellow, blue) }",
      ".foo{background:radial-gradient(5em at 0 0,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(circle at 100%, #333, #333 50%, #eee 75%, #333 75%) }",
      ".foo{background:radial-gradient(circle at 100%,#333,#333 50%,#eee 75%,#333 75%)}",
    );
    minify_test(
      ".foo { background: radial-gradient(farthest-corner circle at 100% 50%, #333, #333 50%, #eee 75%, #333 75%) }",
      ".foo{background:radial-gradient(circle at 100%,#333,#333 50%,#eee 75%,#333 75%)}"
    );
    minify_test(
      ".foo { background: radial-gradient(farthest-corner circle at 50% 50%, #333, #333 50%, #eee 75%, #333 75%) }",
      ".foo{background:radial-gradient(circle,#333,#333 50%,#eee 75%,#333 75%)}"
    );
    minify_test(
      ".foo { background: radial-gradient(ellipse at top, #e66465, transparent) }",
      ".foo{background:radial-gradient(at top,#e66465,#0000)}",
    );
    minify_test(
      ".foo { background: radial-gradient(20px, yellow, blue) }",
      ".foo{background:radial-gradient(20px,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(circle 20px, yellow, blue) }",
      ".foo{background:radial-gradient(20px,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(20px 40px, yellow, blue) }",
      ".foo{background:radial-gradient(20px 40px,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(ellipse 20px 40px, yellow, blue) }",
      ".foo{background:radial-gradient(20px 40px,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(ellipse calc(20px + 10px) 40px, yellow, blue) }",
      ".foo{background:radial-gradient(30px 40px,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(circle farthest-side, yellow, blue) }",
      ".foo{background:radial-gradient(circle farthest-side,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(farthest-side circle, yellow, blue) }",
      ".foo{background:radial-gradient(circle farthest-side,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(ellipse farthest-side, yellow, blue) }",
      ".foo{background:radial-gradient(farthest-side,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: radial-gradient(farthest-side ellipse, yellow, blue) }",
      ".foo{background:radial-gradient(farthest-side,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -webkit-radial-gradient(yellow, blue) }",
      ".foo{background:-webkit-radial-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -moz-radial-gradient(yellow, blue) }",
      ".foo{background:-moz-radial-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -o-radial-gradient(yellow, blue) }",
      ".foo{background:-o-radial-gradient(#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: repeating-radial-gradient(circle 20px, yellow, blue) }",
      ".foo{background:repeating-radial-gradient(20px,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -webkit-repeating-radial-gradient(circle 20px, yellow, blue) }",
      ".foo{background:-webkit-repeating-radial-gradient(20px,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -moz-repeating-radial-gradient(circle 20px, yellow, blue) }",
      ".foo{background:-moz-repeating-radial-gradient(20px,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -o-repeating-radial-gradient(circle 20px, yellow, blue) }",
      ".foo{background:-o-repeating-radial-gradient(20px,#ff0,#00f)}",
    );
    minify_test(
      ".foo { background: -webkit-gradient(radial, center center, 0, center center, 100, from(blue), to(yellow)) }",
      ".foo{background:-webkit-gradient(radial,50% 50%,0,50% 50%,100,from(#00f),to(#ff0))}"
    );
    minify_test(
      ".foo { background: conic-gradient(#f06, gold) }",
      ".foo{background:conic-gradient(#f06,gold)}",
    );
    minify_test(
      ".foo { background: conic-gradient(at 50% 50%, #f06, gold) }",
      ".foo{background:conic-gradient(#f06,gold)}",
    );
    minify_test(
      ".foo { background: conic-gradient(from 0deg, #f06, gold) }",
      ".foo{background:conic-gradient(#f06,gold)}",
    );
    minify_test(
      ".foo { background: conic-gradient(from 0, #f06, gold) }",
      ".foo{background:conic-gradient(#f06,gold)}",
    );
    minify_test(
      ".foo { background: conic-gradient(from 0deg at center, #f06, gold) }",
      ".foo{background:conic-gradient(#f06,gold)}",
    );
    minify_test(
      ".foo { background: conic-gradient(white -50%, black 150%) }",
      ".foo{background:conic-gradient(#fff -50%,#000 150%)}",
    );
    minify_test(
      ".foo { background: conic-gradient(white -180deg, black 540deg) }",
      ".foo{background:conic-gradient(#fff -180deg,#000 540deg)}",
    );
    minify_test(
      ".foo { background: conic-gradient(from 45deg, white, black, white) }",
      ".foo{background:conic-gradient(from 45deg,#fff,#000,#fff)}",
    );
    minify_test(
      ".foo { background: repeating-conic-gradient(from 45deg, white, black, white) }",
      ".foo{background:repeating-conic-gradient(from 45deg,#fff,#000,#fff)}",
    );
    minify_test(
      ".foo { background: repeating-conic-gradient(black 0deg 25%, white 0deg 50%) }",
      ".foo{background:repeating-conic-gradient(#000 0deg 25%,#fff 0deg 50%)}",
    );

    test(
      r#"
        .foo {
          background: -webkit-gradient(linear, left top, left bottom, from(red), to(blue));
          background: -webkit-linear-gradient(red, blue);
          background: -moz-linear-gradient(red, blue);
          background: -o-linear-gradient(red, blue);
          background: linear-gradient(red, blue);
        }
      "#,
      indoc! {r#"
        .foo {
          background: -webkit-gradient(linear, left top, left bottom, from(red), to(#00f));
          background: -webkit-linear-gradient(red, #00f);
          background: -moz-linear-gradient(red, #00f);
          background: -o-linear-gradient(red, #00f);
          background: linear-gradient(red, #00f);
        }
      "#},
    );

    prefix_test(
      r#"
      .foo {
        background: -webkit-gradient(linear, left top, left bottom, from(red), to(blue));
        background: -webkit-linear-gradient(red, blue);
        background: -moz-linear-gradient(red, blue);
        background: -o-linear-gradient(red, blue);
        background: linear-gradient(red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background: linear-gradient(red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: -webkit-gradient(linear, left top, left bottom, from(red), to(blue));
        background: -webkit-linear-gradient(red, blue);
        background: -moz-linear-gradient(red, blue);
        background: -o-linear-gradient(red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background: -webkit-gradient(linear, left top, left bottom, from(red), to(#00f));
        background: -webkit-linear-gradient(red, #00f);
        background: -moz-linear-gradient(red, #00f);
        background: -o-linear-gradient(red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-gradient(linear, 0 0, 0 100%, from(red), to(#00f));
        background-image: -webkit-linear-gradient(red, #00f);
        background-image: linear-gradient(red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(to right, red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-gradient(linear, 0 0, 100% 0, from(red), to(#00f));
        background-image: -webkit-linear-gradient(right, red, #00f);
        background-image: linear-gradient(to right, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(to top, red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-gradient(linear, 0 100%, 0 0, from(red), to(#00f));
        background-image: -webkit-linear-gradient(top, red, #00f);
        background-image: linear-gradient(to top, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(to left, red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-gradient(linear, 100% 0, 0 0, from(red), to(#00f));
        background-image: -webkit-linear-gradient(left, red, #00f);
        background-image: linear-gradient(to left, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(to left bottom, red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-gradient(linear, 100% 0, 0 100%, from(red), to(#00f));
        background-image: -webkit-linear-gradient(bottom left, red, #00f);
        background-image: linear-gradient(to bottom left, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(to top right, red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-gradient(linear, 0 100%, 100% 0, from(red), to(#00f));
        background-image: -webkit-linear-gradient(top right, red, #00f);
        background-image: linear-gradient(to top right, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(90deg, red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-gradient(linear, 0 0, 100% 0, from(red), to(#00f));
        background-image: -webkit-linear-gradient(90deg, red, #00f);
        background-image: linear-gradient(90deg, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(45deg, red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-linear-gradient(45deg, red, #00f);
        background-image: linear-gradient(45deg, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: linear-gradient(red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-linear-gradient(red, #00f);
        background-image: linear-gradient(red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: radial-gradient(20px, red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-gradient(radial, center center, 0, center center, 20, from(red), to(#00f));
        background-image: -webkit-radial-gradient(20px, red, #00f);
        background-image: radial-gradient(20px, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: radial-gradient(20px at top left, red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-gradient(radial, left top, 0, left top, 20, from(red), to(#00f));
        background-image: -webkit-radial-gradient(20px at 0 0, red, #00f);
        background-image: radial-gradient(20px at 0 0, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: radial-gradient(red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: -webkit-radial-gradient(red, #00f);
        background-image: radial-gradient(red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background-image: -webkit-gradient(radial, left top, 0, left top, 20, from(red), to(#00f));
        background-image: -webkit-radial-gradient(20px at 0% 0%, red, #00f);
        background-image: radial-gradient(20px at 0% 0%, red, #00f);
      }
      "#,
      indoc! {r#"
      .foo {
        background-image: radial-gradient(20px at 0 0, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(30 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: -webkit-gradient(radial, left top, 0, left top, 20, from(red), to(#00f));
        background: -webkit-radial-gradient(20px at 0% 0%, red, #00f);
        background: radial-gradient(20px at 0% 0%, red, #00f);
      }
      "#,
      indoc! {r#"
      .foo {
        background: radial-gradient(20px at 0 0, red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(30 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: radial-gradient(red, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background: -webkit-radial-gradient(red, #00f);
        background: radial-gradient(red, #00f);
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        background: radial-gradient(red, blue), linear-gradient(yellow, red), url(bg.jpg);
      }
      "#,
      indoc! {r#"
      .foo {
        background: -webkit-gradient(linear, 0 0, 0 100%, from(#ff0), to(red)), url("bg.jpg");
        background: -webkit-radial-gradient(red, #00f), -webkit-linear-gradient(#ff0, red), url("bg.jpg");
        background: -moz-radial-gradient(red, #00f), -moz-linear-gradient(#ff0, red), url("bg.jpg");
        background: -o-radial-gradient(red, #00f), -o-linear-gradient(#ff0, red), url("bg.jpg");
        background: radial-gradient(red, #00f), linear-gradient(#ff0, red), url("bg.jpg");
      }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        firefox: Some(4 << 16),
        opera: Some(11 << 16 | 5 << 8),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background: linear-gradient(yellow, red 30% 40%, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background: linear-gradient(#ff0, red 30%, red 40%, #00f);
      }
      "#},
      Browsers {
        chrome: Some(70 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background: linear-gradient(yellow, red 30% 40%, blue);
      }
      "#,
      indoc! {r#"
      .foo {
        background: linear-gradient(#ff0, red 30% 40%, #00f);
      }
      "#},
      Browsers {
        chrome: Some(71 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          background: linear-gradient(#ff0f0e, #7773ff);
          background: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          background: linear-gradient(#ff0f0e, #7773ff);
          background: linear-gradient(color(display-p3 1 .0000153435 -.00000303562), color(display-p3 .440289 .28452 1.23485));
          background: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          background: -webkit-linear-gradient(#ff0f0e, #7773ff);
          background: linear-gradient(#ff0f0e, #7773ff);
          background: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(20 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          background: -webkit-gradient(linear, 0 0, 0 100%, from(#ff0f0e), to(#7773ff));
          background: -webkit-linear-gradient(#ff0f0e, #7773ff);
          background: linear-gradient(#ff0f0e, #7773ff);
          background: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          background: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-image: linear-gradient(oklab(59.686% 0.1009 0.1192), oklab(54.0% -0.10 -0.02)); }",
      indoc! { r#"
        .foo {
          background-image: linear-gradient(lab(52.2319% 40.1449 59.9171), lab(47.7776% -34.2947 -7.65904));
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          background-image: linear-gradient(#ff0f0e, #7773ff);
          background-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          background-image: linear-gradient(#ff0f0e, #7773ff);
          background-image: linear-gradient(color(display-p3 1 .0000153435 -.00000303562), color(display-p3 .440289 .28452 1.23485));
          background-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          background-image: -webkit-linear-gradient(#ff0f0e, #7773ff);
          background-image: linear-gradient(#ff0f0e, #7773ff);
          background-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(20 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          background-image: -webkit-gradient(linear, 0 0, 0 100%, from(#ff0f0e), to(#7773ff));
          background-image: -webkit-linear-gradient(#ff0f0e, #7773ff);
          background-image: linear-gradient(#ff0f0e, #7773ff);
          background-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          background-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-image: linear-gradient(oklab(59.686% 0.1009 0.1192), oklab(54.0% -0.10 -0.02)); }",
      indoc! { r#"
        .foo {
          background-image: linear-gradient(lab(52.2319% 40.1449 59.9171), lab(47.7776% -34.2947 -7.65904));
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_font_face() {
    minify_test(
      r#"@font-face {
      src: url("test.woff");
      font-family: "Helvetica";
      font-weight: bold;
      font-style: italic;
    }"#,
      "@font-face{src:url(test.woff);font-family:Helvetica;font-weight:700;font-style:italic}",
    );
    minify_test("@font-face {src: url(test.woff);}", "@font-face{src:url(test.woff)}");
    minify_test("@font-face {src: local(\"Test\");}", "@font-face{src:local(Test)}");
    minify_test(
      "@font-face {src: local(\"Foo Bar\");}",
      "@font-face{src:local(Foo Bar)}",
    );
    minify_test("@font-face {src: local(Test);}", "@font-face{src:local(Test)}");
    minify_test("@font-face {src: local(Foo Bar);}", "@font-face{src:local(Foo Bar)}");

    minify_test(
      "@font-face {src: url(\"test.woff\") format(woff);}",
      "@font-face{src:url(test.woff)format(\"woff\")}",
    );
    minify_test(
      "@font-face {src: url(\"test.ttc\") format(collection), url(test.ttf) format(truetype);}",
      "@font-face{src:url(test.ttc)format(\"collection\"),url(test.ttf)format(\"truetype\")}",
    );
    minify_test(
      "@font-face {src: url(\"test.otf\") format(opentype) tech(features-aat);}",
      "@font-face{src:url(test.otf)format(\"opentype\")tech(features-aat)}",
    );
    minify_test(
      "@font-face {src: url(\"test.woff\") format(woff) tech(color-colrv1);}",
      "@font-face{src:url(test.woff)format(\"woff\")tech(color-colrv1)}",
    );
    minify_test(
      "@font-face {src: url(\"test.woff2\") format(woff2) tech(variations);}",
      "@font-face{src:url(test.woff2)format(\"woff2\")tech(variations)}",
    );
    minify_test(
      "@font-face {src: url(\"test.woff\") format(woff) tech(palettes);}",
      "@font-face{src:url(test.woff)format(\"woff\")tech(palettes)}",
    );
    // multiple tech
    minify_test(
      "@font-face {src: url(\"test.woff\") format(woff) tech(features-opentype, color-sbix);}",
      "@font-face{src:url(test.woff)format(\"woff\")tech(features-opentype,color-sbix)}",
    );
    minify_test(
      "@font-face {src: url(\"test.woff\")   format(woff)    tech(incremental, color-svg, features-graphite, features-aat);}",
      "@font-face{src:url(test.woff)format(\"woff\")tech(incremental,color-svg,features-graphite,features-aat)}",
    );
    // format() function must precede tech() if both are present
    minify_test(
      "@font-face {src: url(\"foo.ttf\") format(opentype) tech(color-colrv1);}",
      "@font-face{src:url(foo.ttf)format(\"opentype\")tech(color-colrv1)}",
    );
    // only have tech is valid
    minify_test(
      "@font-face {src: url(\"foo.ttf\") tech(color-SVG);}",
      "@font-face{src:url(foo.ttf)tech(color-svg)}",
    );
    // CGQAQ: if tech and format both presence, order is matter, tech before format is invalid
    // but now just return raw token, we don't have strict mode yet.
    // ref: https://github.com/parcel-bundler/lightningcss/pull/255#issuecomment-1219049998
    minify_test(
      "@font-face {src: url(\"foo.ttf\") tech(palettes  color-colrv0  variations) format(opentype);}",
      "@font-face{src:url(foo.ttf) tech(palettes color-colrv0 variations)format(opentype)}",
    );
    // TODO(CGQAQ): make this test pass when we have strict mode
    // ref: https://github.com/web-platform-tests/wpt/blob/9f8a6ccc41aa725e8f51f4f096f686313bb88d8d/css/css-fonts/parsing/font-face-src-tech.html#L45
    // error_test(
    //   "@font-face {src: url(\"foo.ttf\") tech(features-opentype) format(opentype);}",
    //   ParserError::AtRuleBodyInvalid,
    // );
    // error_test(
    //   "@font-face {src: url(\"foo.ttf\") tech();}",
    //   ParserError::AtRuleBodyInvalid,
    // );
    // error_test(
    //   "@font-face {src: url(\"foo.ttf\") tech(\"features-opentype\");}",
    //   ParserError::AtRuleBodyInvalid,
    // );
    // error_test(
    //   "@font-face {src: url(\"foo.ttf\") tech(\"color-colrv0\");}",
    //   ParserError::AtRuleBodyInvalid,
    // );
    minify_test(
      "@font-face {src: local(\"\") url(\"test.woff\");}",
      "@font-face{src:local(\"\")url(test.woff)}",
    );
    minify_test("@font-face {font-weight: 200 400}", "@font-face{font-weight:200 400}");
    minify_test("@font-face {font-weight: 400 400}", "@font-face{font-weight:400}");
    minify_test(
      "@font-face {font-stretch: 50% 200%}",
      "@font-face{font-stretch:50% 200%}",
    );
    minify_test("@font-face {font-stretch: 50% 50%}", "@font-face{font-stretch:50%}");
    minify_test("@font-face {unicode-range: U+26;}", "@font-face{unicode-range:U+26}");
    minify_test("@font-face {unicode-range: u+26;}", "@font-face{unicode-range:U+26}");
    minify_test(
      "@font-face {unicode-range: U+0-7F;}",
      "@font-face{unicode-range:U+0-7F}",
    );
    minify_test(
      "@font-face {unicode-range: U+0025-00FF;}",
      "@font-face{unicode-range:U+25-FF}",
    );
    minify_test("@font-face {unicode-range: U+4??;}", "@font-face{unicode-range:U+4??}");
    minify_test(
      "@font-face {unicode-range: U+400-4FF;}",
      "@font-face{unicode-range:U+4??}",
    );
    minify_test(
      "@font-face {unicode-range: U+0025-00FF, U+4??;}",
      "@font-face{unicode-range:U+25-FF,U+4??}",
    );
    minify_test(
      "@font-face {unicode-range: U+A5, U+4E00-9FFF, U+30??, U+FF00-FF9F;}",
      "@font-face{unicode-range:U+A5,U+4E00-9FFF,U+30??,U+FF00-FF9F}",
    );
    minify_test(
      "@font-face {unicode-range: U+????;}",
      "@font-face{unicode-range:U+????}",
    );
    minify_test(
      "@font-face {unicode-range: U+0000-FFFF;}",
      "@font-face{unicode-range:U+????}",
    );
    minify_test(
      "@font-face {unicode-range: U+10????;}",
      "@font-face{unicode-range:U+10????}",
    );
    minify_test(
      "@font-face {unicode-range: U+100000-10FFFF;}",
      "@font-face{unicode-range:U+10????}",
    );
    minify_test(
      "@font-face {unicode-range: U+1e1e?;}",
      "@font-face{unicode-range:U+1E1E?}",
    );
    minify_test(
      "@font-face {unicode-range: u+????, U+1????, U+10????;}",
      "@font-face{unicode-range:U+????,U+1????,U+10????}",
    );
    minify_test(r#"
      @font-face {
        font-family: Inter;
        font-style: oblique 0deg 10deg;
        font-weight: 100 900;
        src: url("../fonts/Inter.var.woff2?v=3.19") format("woff2");
        font-display: swap;
      }
    "#, "@font-face{font-family:Inter;font-style:oblique 0deg 10deg;font-weight:100 900;src:url(../fonts/Inter.var.woff2?v=3.19)format(\"woff2\");font-display:swap}");
    minify_test(r#"
    @font-face {
      font-family: Inter;
      font-style: oblique 14deg 14deg;
      font-weight: 100 900;
      src: url("../fonts/Inter.var.woff2?v=3.19") format("woff2");
      font-display: swap;
    }
  "#, "@font-face{font-family:Inter;font-style:oblique;font-weight:100 900;src:url(../fonts/Inter.var.woff2?v=3.19)format(\"woff2\");font-display:swap}");
  }

  #[test]
  fn test_font_palette_values() {
    minify_test(
      r#"@font-palette-values --Cooler {
      font-family: Bixa;
      base-palette: 1;
      override-colors: 1 #7EB7E4;
    }"#,
      "@font-palette-values --Cooler{font-family:Bixa;base-palette:1;override-colors:1 #7eb7e4}",
    );
    minify_test(
      r#"@font-palette-values --Cooler {
      font-family: Handover Sans;
      base-palette: 3;
      override-colors: 1 rgb(43, 12, 9), 3 lime;
    }"#,
      "@font-palette-values --Cooler{font-family:Handover Sans;base-palette:3;override-colors:1 #2b0c09,3 #0f0}",
    );
    minify_test(r#"@font-palette-values --Cooler {
      font-family: Handover Sans;
      base-palette: 3;
      override-colors: 1 rgb(43, 12, 9), 3 var(--highlight);
    }"#, "@font-palette-values --Cooler{font-family:Handover Sans;base-palette:3;override-colors:1 #2b0c09,3 var(--highlight)}");
    prefix_test(
      r#"@font-palette-values --Cooler {
      font-family: Handover Sans;
      base-palette: 3;
      override-colors: 1 rgb(43, 12, 9), 3 lch(50.998% 135.363 338);
    }"#,
      indoc! {r#"@font-palette-values --Cooler {
      font-family: Handover Sans;
      base-palette: 3;
      override-colors: 1 #2b0c09, 3 #ee00be;
      override-colors: 1 #2b0c09, 3 lch(50.998% 135.363 338);
    }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"@font-palette-values --Cooler {
      font-family: Handover Sans;
      base-palette: 3;
      override-colors: 1 var(--foo), 3 lch(50.998% 135.363 338);
    }"#,
      indoc! {r#"@font-palette-values --Cooler {
      font-family: Handover Sans;
      base-palette: 3;
      override-colors: 1 var(--foo), 3 #ee00be;
    }

    @supports (color: lab(0% 0 0)) {
      @font-palette-values --Cooler {
        font-family: Handover Sans;
        base-palette: 3;
        override-colors: 1 var(--foo), 3 lab(50.998% 125.506 -50.7078);
      }
    }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    minify_test(".foo { font-palette: --Custom; }", ".foo{font-palette:--Custom}");
  }

  #[test]
  fn test_font_feature_values() {
    // https://github.com/clagnut/TODS/blob/e693d52ad411507b960cf01a9734265e3efab102/tods.css#L116-L142
    minify_test(
      r#"
@font-feature-values "Fancy Font Name" {
  @styleset { cursive: 1; swoopy: 7 16; }
  @character-variant { ampersand: 1; capital-q: 2; }
  @stylistic { two-story-g: 1; straight-y: 2; }
  @swash { swishy: 1; flowing: 2; }
  @ornaments { clover: 1; fleuron: 2; }
  @annotation { circled: 1; boxed: 2; }
}
    "#,
      r#"@font-feature-values Fancy Font Name{@styleset{cursive:1;swoopy:7 16}@character-variant{ampersand:1;capital-q:2}@stylistic{two-story-g:1;straight-y:2}@swash{swishy:1;flowing:2}@ornaments{clover:1;fleuron:2}@annotation{circled:1;boxed:2}}"#,
    );

    // https://github.com/Sorixelle/srxl.me/blob/4eb4f4a15cb2d21356df24c096d6a819cfdc1a99/public/fonts/inter/inter.css#L201-L222
    minify_test(
      r#"
@font-feature-values "Inter", "Inter var", "Inter var experimental" {
  @styleset {
    open-digits: 1;
    disambiguation: 2;
    curved-r: 3;
    disambiguation-without-zero: 4;
  }

  @character-variant {
    alt-one: 1;
    open-four: 2;
    open-six: 3;
    open-nine: 4;
    lower-l-with-tail: 5;
    curved-lower-r: 6;
    german-double-s: 7;
    upper-i-with-serif: 8;
    flat-top-three: 9;
    upper-g-with-spur: 10;
    single-storey-a: 11;
  }
}
      "#,
      r#"@font-feature-values Inter,Inter var,Inter var experimental{@styleset{open-digits:1;disambiguation:2;curved-r:3;disambiguation-without-zero:4}@character-variant{alt-one:1;open-four:2;open-six:3;open-nine:4;lower-l-with-tail:5;curved-lower-r:6;german-double-s:7;upper-i-with-serif:8;flat-top-three:9;upper-g-with-spur:10;single-storey-a:11}}"#,
    );

    // https://github.com/MihailJP/Inconsolata-LGC/blob/7c53cf455787096c93d82d9a51018f12ec39a6e9/Inconsolata-LGC.css#L65-L91
    minify_test(
      r#"
@font-feature-values "Inconsolata LGC" {
	@styleset {
		alternative-umlaut: 1;
	}
	@character-variant {
		zero-plain: 1 1;
		zero-dotted: 1 2;
		zero-longslash: 1 3;
		r-with-serif: 2 1;
		eng-descender: 3 1;
		eng-uppercase: 3 2;
		dollar-open: 4 1;
		dollar-oldstyle: 4 2;
		dollar-cifrao: 4 2;
		ezh-no-descender: 5 1;
		ezh-reversed-sigma: 5 2;
		triangle-text-form: 6 1;
		el-with-hook-old: 7 1;
		qa-enlarged-lowercase: 8 1;
		qa-reversed-p: 8 2;
		che-with-hook: 9 1;
		che-with-hook-alt: 9 2;
		ge-with-hook: 10 1;
		ge-with-hook-alt: 10 2;
		ge-with-stroke-and-descender: 11 1;
	}
}
    "#,
      r#"@font-feature-values Inconsolata LGC{@styleset{alternative-umlaut:1}@character-variant{zero-plain:1 1;zero-dotted:1 2;zero-longslash:1 3;r-with-serif:2 1;eng-descender:3 1;eng-uppercase:3 2;dollar-open:4 1;dollar-oldstyle:4 2;dollar-cifrao:4 2;ezh-no-descender:5 1;ezh-reversed-sigma:5 2;triangle-text-form:6 1;el-with-hook-old:7 1;qa-enlarged-lowercase:8 1;qa-reversed-p:8 2;che-with-hook:9 1;che-with-hook-alt:9 2;ge-with-hook:10 1;ge-with-hook-alt:10 2;ge-with-stroke-and-descender:11 1}}"#,
    );

    minify_test(
      r#"
      @font-feature-values "Fancy Font Name" {
        @styleset { cursive: 1; swoopy: 7 16; }
        @character-variant { ampersand: 1; capital-q: 2; }
      }
      "#,
      r#"@font-feature-values Fancy Font Name{@styleset{cursive:1;swoopy:7 16}@character-variant{ampersand:1;capital-q:2}}"#,
    );
    minify_test(
      r#"
      @font-feature-values foo {
          @swash { pretty: 0; pretty: 1; cool: 2; }
      }
      "#,
      "@font-feature-values foo{@swash{pretty:1;cool:2}}",
    );
    minify_test(
      r#"
      @font-feature-values foo {
          @swash { pretty: 1; }
          @swash { cool: 2; }
      }
      "#,
      "@font-feature-values foo{@swash{pretty:1;cool:2}}",
    );
    minify_test(
      r#"
      @font-feature-values foo {
          @swash { pretty: 1; }
      }
      @font-feature-values foo {
          @swash { cool: 2; }
      }
      "#,
      "@font-feature-values foo{@swash{pretty:1;cool:2}}",
    );
  }

  #[test]
  fn test_page_rule() {
    minify_test("@page {margin: 0.5cm}", "@page{margin:.5cm}");
    minify_test("@page :left {margin: 0.5cm}", "@page:left{margin:.5cm}");
    minify_test("@page :right {margin: 0.5cm}", "@page:right{margin:.5cm}");
    minify_test(
      "@page LandscapeTable {margin: 0.5cm}",
      "@page LandscapeTable{margin:.5cm}",
    );
    minify_test(
      "@page CompanyLetterHead:first {margin: 0.5cm}",
      "@page CompanyLetterHead:first{margin:.5cm}",
    );
    minify_test("@page:first {margin: 0.5cm}", "@page:first{margin:.5cm}");
    minify_test("@page :blank:first {margin: 0.5cm}", "@page:blank:first{margin:.5cm}");
    minify_test("@page toc, index {margin: 0.5cm}", "@page toc,index{margin:.5cm}");
    minify_test(
      r#"
    @page :right {
      @bottom-left {
        margin: 10pt;
      }
    }
    "#,
      "@page:right{@bottom-left{margin:10pt}}",
    );
    minify_test(
      r#"
    @page :right {
      margin: 1in;

      @bottom-left {
        margin: 10pt;
      }
    }
    "#,
      "@page:right{margin:1in;@bottom-left{margin:10pt}}",
    );

    test(
      r#"
    @page :right {
      @bottom-left {
        margin: 10pt;
      }
    }
    "#,
      indoc! {r#"
      @page :right {
        @bottom-left {
          margin: 10pt;
        }
      }
      "#},
    );

    test(
      r#"
    @page :right {
      margin: 1in;

      @bottom-left-corner { content: "Foo"; }
      @bottom-right-corner { content: "Bar"; }
    }
    "#,
      indoc! {r#"
      @page :right {
        margin: 1in;

        @bottom-left-corner {
          content: "Foo";
        }

        @bottom-right-corner {
          content: "Bar";
        }
      }
      "#},
    );

    error_test(
      r#"
      @page {
        @foo {
          margin: 1in;
        }
      }
      "#,
      ParserError::AtRuleInvalid("foo".into()),
    );

    error_test(
      r#"
      @page {
        @top-left-corner {
          @bottom-left {
            margin: 1in;
          }
        }
      }
      "#,
      ParserError::AtRuleInvalid("bottom-left".into()),
    );
  }

  #[test]
  fn test_supports_rule() {
    test(
      r#"
      @supports (foo: bar) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports (foo: bar) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports not (foo: bar) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports not (foo: bar) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports (foo: bar) or (bar: baz) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports (foo: bar) or (bar: baz) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports (((foo: bar) or (bar: baz))) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports (foo: bar) or (bar: baz) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports (foo: bar) and (bar: baz) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports (foo: bar) and (bar: baz) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports (((foo: bar) and (bar: baz))) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports (foo: bar) and (bar: baz) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports (foo: bar) and (((bar: baz) or (test: foo))) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports (foo: bar) and ((bar: baz) or (test: foo)) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports not (((foo: bar) and (bar: baz))) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports not ((foo: bar) and (bar: baz)) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports selector(a > b) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports selector(a > b) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports unknown(test) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports unknown(test) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports (unknown) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports (unknown) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    test(
      r#"
      @supports (display: grid) and (not (display: inline-grid)) {
        .test {
          foo: bar;
        }
      }
    "#,
      indoc! { r#"
      @supports (display: grid) and (not (display: inline-grid)) {
        .test {
          foo: bar;
        }
      }
    "#},
    );
    prefix_test(
      r#"
      @supports (backdrop-filter: blur(10px)) {
        div {
          backdrop-filter: blur(10px);
        }
      }
    "#,
      indoc! { r#"
      @supports ((-webkit-backdrop-filter: blur(10px)) or (backdrop-filter: blur(10px))) {
        div {
          -webkit-backdrop-filter: blur(10px);
          backdrop-filter: blur(10px);
        }
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        ..Default::default()
      },
    );
    prefix_test(
      r#"
      @supports ((-webkit-backdrop-filter: blur(10px)) or (backdrop-filter: blur(10px))) {
        div {
          backdrop-filter: blur(10px);
        }
      }
    "#,
      indoc! { r#"
      @supports ((-webkit-backdrop-filter: blur(10px)) or (backdrop-filter: blur(10px))) {
        div {
          -webkit-backdrop-filter: blur(10px);
          backdrop-filter: blur(10px);
        }
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        ..Default::default()
      },
    );
    prefix_test(
      r#"
      @supports ((-webkit-backdrop-filter: blur(20px)) or (backdrop-filter: blur(10px))) {
        div {
          backdrop-filter: blur(10px);
        }
      }
    "#,
      indoc! { r#"
      @supports ((-webkit-backdrop-filter: blur(20px))) or ((-webkit-backdrop-filter: blur(10px)) or (backdrop-filter: blur(10px))) {
        div {
          -webkit-backdrop-filter: blur(10px);
          backdrop-filter: blur(10px);
        }
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        ..Default::default()
      },
    );
    prefix_test(
      r#"
      @supports ((-webkit-backdrop-filter: blur(10px)) or (backdrop-filter: blur(10px))) {
        div {
          backdrop-filter: blur(10px);
        }
      }
    "#,
      indoc! { r#"
      @supports (backdrop-filter: blur(10px)) {
        div {
          backdrop-filter: blur(10px);
        }
      }
    "#},
      Browsers {
        chrome: Some(80 << 16),
        ..Default::default()
      },
    );
    minify_test(
      r#"
      @supports (width: calc(10px * 2)) {
        .test {
          width: calc(10px * 2);
        }
      }
    "#,
      "@supports (width:calc(10px * 2)){.test{width:20px}}",
    );
    minify_test(
      r#"
      @supports (color: hsl(0deg, 0%, 0%)) {
        .test {
          color: hsl(0deg, 0%, 0%);
        }
      }
    "#,
      "@supports (color:hsl(0deg, 0%, 0%)){.test{color:#000}}",
    );
  }

  #[test]
  fn test_counter_style() {
    test(
      r#"
      @counter-style circled-alpha {
        system: fixed;
        symbols: Ⓐ Ⓑ Ⓒ;
        suffix: " ";
      }
    "#,
      indoc! { r#"
      @counter-style circled-alpha {
        system: fixed;
        symbols: Ⓐ Ⓑ Ⓒ;
        suffix: " ";
      }
    "#},
    );
  }

  #[test]
  fn test_namespace() {
    minify_test(
      "@namespace url(http://toto.example.org);",
      "@namespace \"http://toto.example.org\";",
    );
    minify_test(
      "@namespace \"http://toto.example.org\";",
      "@namespace \"http://toto.example.org\";",
    );
    minify_test(
      "@namespace toto \"http://toto.example.org\";",
      "@namespace toto \"http://toto.example.org\";",
    );
    minify_test(
      "@namespace toto url(http://toto.example.org);",
      "@namespace toto \"http://toto.example.org\";",
    );

    test(
      r#"
      @namespace "http://example.com/foo";

      x {
        color: red;
      }
    "#,
      indoc! {r#"
      @namespace "http://example.com/foo";

      x {
        color: red;
      }
    "#},
    );

    test(
      r#"
      @namespace toto "http://toto.example.org";

      toto|x {
        color: red;
      }

      [toto|att=val] {
        color: blue
      }
    "#,
      indoc! {r#"
      @namespace toto "http://toto.example.org";

      toto|x {
        color: red;
      }

      [toto|att="val"] {
        color: #00f;
      }
    "#},
    );

    test(
      r#"
      @namespace "http://example.com/foo";

      |x {
        color: red;
      }

      [|att=val] {
        color: blue
      }
    "#,
      indoc! {r#"
      @namespace "http://example.com/foo";

      |x {
        color: red;
      }

      [att="val"] {
        color: #00f;
      }
    "#},
    );

    test(
      r#"
      @namespace "http://example.com/foo";

      *|x {
        color: red;
      }

      [*|att=val] {
        color: blue
      }
    "#,
      indoc! {r#"
      @namespace "http://example.com/foo";

      *|x {
        color: red;
      }

      [*|att="val"] {
        color: #00f;
      }
    "#},
    );

    error_test(
      ".foo { color: red } @namespace \"http://example.com/foo\";",
      ParserError::UnexpectedNamespaceRule,
    );
  }

  #[test]
  fn test_import() {
    minify_test("@import url(foo.css);", "@import \"foo.css\";");
    minify_test("@import \"foo.css\";", "@import \"foo.css\";");
    minify_test("@import url(foo.css) print;", "@import \"foo.css\" print;");
    minify_test("@import \"foo.css\" print;", "@import \"foo.css\" print;");
    minify_test(
      "@import \"foo.css\" screen and (orientation: landscape);",
      "@import \"foo.css\" screen and (orientation:landscape);",
    );
    minify_test(
      "@import url(foo.css) supports(display: flex);",
      "@import \"foo.css\" supports(display:flex);",
    );
    minify_test(
      "@import url(foo.css) supports(display: flex) print;",
      "@import \"foo.css\" supports(display:flex) print;",
    );
    minify_test(
      "@import url(foo.css) supports(not (display: flex));",
      "@import \"foo.css\" supports(not (display:flex));",
    );
    minify_test(
      "@import url(foo.css) supports((display: flex));",
      "@import \"foo.css\" supports(display:flex);",
    );
    minify_test("@charset \"UTF-8\"; @import url(foo.css);", "@import \"foo.css\";");
    minify_test("@layer foo; @import url(foo.css);", "@layer foo;@import \"foo.css\";");
    error_test(
      ".foo { color: red } @import url(bar.css);",
      ParserError::UnexpectedImportRule,
    );
    error_test(
      "@namespace \"http://example.com/foo\"; @import url(bar.css);",
      ParserError::UnexpectedImportRule,
    );
    error_test(
      "@media print { .foo { color: red }} @import url(bar.css);",
      ParserError::UnexpectedImportRule,
    );
    error_test(
      "@layer foo; @import url(foo.css); @layer bar; @import url(bar.css)",
      ParserError::UnexpectedImportRule,
    );
  }

  #[test]
  fn test_prefixes() {
    prefix_test(
      r#"
      .foo {
        -webkit-transition: opacity 200ms;
        -moz-transition: opacity 200ms;
        transition: opacity 200ms;
      }
      "#,
      indoc! {r#"
      .foo {
        transition: opacity .2s;
      }
      "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo{transition:opacity 200ms}",
      indoc! {r#"
      .foo {
        -webkit-transition: opacity .2s;
        -moz-transition: opacity .2s;
        transition: opacity .2s;
      }
      "#},
      Browsers {
        safari: Some(5 << 16),
        firefox: Some(14 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_display() {
    minify_test(".foo { display: block }", ".foo{display:block}");
    minify_test(".foo { display: block flow }", ".foo{display:block}");
    minify_test(".foo { display: flow-root }", ".foo{display:flow-root}");
    minify_test(".foo { display: block flow-root }", ".foo{display:flow-root}");
    minify_test(".foo { display: inline }", ".foo{display:inline}");
    minify_test(".foo { display: inline flow }", ".foo{display:inline}");
    minify_test(".foo { display: inline-block }", ".foo{display:inline-block}");
    minify_test(".foo { display: inline flow-root }", ".foo{display:inline-block}");
    minify_test(".foo { display: run-in }", ".foo{display:run-in}");
    minify_test(".foo { display: run-in flow }", ".foo{display:run-in}");
    minify_test(".foo { display: list-item }", ".foo{display:list-item}");
    minify_test(".foo { display: block flow list-item }", ".foo{display:list-item}");
    minify_test(".foo { display: inline list-item }", ".foo{display:inline list-item}");
    minify_test(
      ".foo { display: inline flow list-item }",
      ".foo{display:inline list-item}",
    );
    minify_test(".foo { display: flex }", ".foo{display:flex}");
    minify_test(".foo { display: block flex }", ".foo{display:flex}");
    minify_test(".foo { display: inline-flex }", ".foo{display:inline-flex}");
    minify_test(".foo { display: inline flex }", ".foo{display:inline-flex}");
    minify_test(".foo { display: grid }", ".foo{display:grid}");
    minify_test(".foo { display: block grid }", ".foo{display:grid}");
    minify_test(".foo { display: inline-grid }", ".foo{display:inline-grid}");
    minify_test(".foo { display: inline grid }", ".foo{display:inline-grid}");
    minify_test(".foo { display: ruby }", ".foo{display:ruby}");
    minify_test(".foo { display: inline ruby }", ".foo{display:ruby}");
    minify_test(".foo { display: block ruby }", ".foo{display:block ruby}");
    minify_test(".foo { display: table }", ".foo{display:table}");
    minify_test(".foo { display: block table }", ".foo{display:table}");
    minify_test(".foo { display: inline-table }", ".foo{display:inline-table}");
    minify_test(".foo { display: inline table }", ".foo{display:inline-table}");
    minify_test(".foo { display: table-row-group }", ".foo{display:table-row-group}");
    minify_test(".foo { display: contents }", ".foo{display:contents}");
    minify_test(".foo { display: none }", ".foo{display:none}");
    minify_test(".foo { display: -webkit-flex }", ".foo{display:-webkit-flex}");
    minify_test(".foo { display: -ms-flexbox }", ".foo{display:-ms-flexbox}");
    minify_test(".foo { display: -webkit-box }", ".foo{display:-webkit-box}");
    minify_test(".foo { display: -moz-box }", ".foo{display:-moz-box}");
    minify_test(
      ".foo { display: -webkit-flex; display: -moz-box; display: flex }",
      ".foo{display:-webkit-flex;display:-moz-box;display:flex}",
    );
    minify_test(
      ".foo { display: -webkit-flex; display: flex; display: -moz-box }",
      ".foo{display:-webkit-flex;display:flex;display:-moz-box}",
    );
    minify_test(".foo { display: flex; display: grid }", ".foo{display:grid}");
    minify_test(
      ".foo { display: -webkit-inline-flex; display: -moz-inline-box; display: inline-flex }",
      ".foo{display:-webkit-inline-flex;display:-moz-inline-box;display:inline-flex}",
    );
    minify_test(
      ".foo { display: flex; display: var(--grid); }",
      ".foo{display:flex;display:var(--grid)}",
    );
    prefix_test(
      ".foo{ display: flex }",
      indoc! {r#"
      .foo {
        display: -webkit-box;
        display: -moz-box;
        display: -webkit-flex;
        display: -ms-flexbox;
        display: flex;
      }
      "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(14 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo{ display: flex; display: -webkit-box; }",
      indoc! {r#"
      .foo {
        display: -webkit-box;
      }
      "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(14 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo{ display: -webkit-box; display: flex; }",
      indoc! {r#"
      .foo {
        display: -webkit-box;
        display: -moz-box;
        display: -webkit-flex;
        display: -ms-flexbox;
        display: flex;
      }
      "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(14 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        display: -webkit-box;
        display: -moz-box;
        display: -webkit-flex;
        display: -ms-flexbox;
        display: flex;
      }
      "#,
      indoc! {r#"
      .foo {
        display: flex;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        display: -webkit-box;
        display: flex;
        display: -moz-box;
        display: -webkit-flex;
        display: -ms-flexbox;
      }
      "#,
      indoc! {r#"
      .foo {
        display: -moz-box;
        display: -webkit-flex;
        display: -ms-flexbox;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo{ display: inline-flex }",
      indoc! {r#"
      .foo {
        display: -webkit-inline-box;
        display: -moz-inline-box;
        display: -webkit-inline-flex;
        display: -ms-inline-flexbox;
        display: inline-flex;
      }
      "#},
      Browsers {
        safari: Some(4 << 16),
        firefox: Some(14 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        display: -webkit-inline-box;
        display: -moz-inline-box;
        display: -webkit-inline-flex;
        display: -ms-inline-flexbox;
        display: inline-flex;
      }
      "#,
      indoc! {r#"
      .foo {
        display: inline-flex;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_visibility() {
    minify_test(".foo { visibility: visible }", ".foo{visibility:visible}");
    minify_test(".foo { visibility: hidden }", ".foo{visibility:hidden}");
    minify_test(".foo { visibility: collapse }", ".foo{visibility:collapse}");
    minify_test(".foo { visibility: Visible }", ".foo{visibility:visible}");
  }

  #[test]
  fn test_text_transform() {
    minify_test(".foo { text-transform: uppercase }", ".foo{text-transform:uppercase}");
    minify_test(".foo { text-transform: lowercase }", ".foo{text-transform:lowercase}");
    minify_test(".foo { text-transform: capitalize }", ".foo{text-transform:capitalize}");
    minify_test(".foo { text-transform: none }", ".foo{text-transform:none}");
    minify_test(".foo { text-transform: full-width }", ".foo{text-transform:full-width}");
    minify_test(
      ".foo { text-transform: full-size-kana }",
      ".foo{text-transform:full-size-kana}",
    );
    minify_test(
      ".foo { text-transform: uppercase full-width }",
      ".foo{text-transform:uppercase full-width}",
    );
    minify_test(
      ".foo { text-transform: full-width uppercase }",
      ".foo{text-transform:uppercase full-width}",
    );
    minify_test(
      ".foo { text-transform: uppercase full-width full-size-kana }",
      ".foo{text-transform:uppercase full-width full-size-kana}",
    );
    minify_test(
      ".foo { text-transform: full-width uppercase full-size-kana }",
      ".foo{text-transform:uppercase full-width full-size-kana}",
    );
  }

  #[test]
  fn test_whitespace() {
    minify_test(".foo { white-space: normal }", ".foo{white-space:normal}");
    minify_test(".foo { white-space: pre }", ".foo{white-space:pre}");
    minify_test(".foo { white-space: nowrap }", ".foo{white-space:nowrap}");
    minify_test(".foo { white-space: pre-wrap }", ".foo{white-space:pre-wrap}");
    minify_test(".foo { white-space: break-spaces }", ".foo{white-space:break-spaces}");
    minify_test(".foo { white-space: pre-line }", ".foo{white-space:pre-line}");
    minify_test(".foo { white-space: NoWrAp }", ".foo{white-space:nowrap}");
  }

  #[test]
  fn test_tab_size() {
    minify_test(".foo { tab-size: 8 }", ".foo{tab-size:8}");
    minify_test(".foo { tab-size: 4px }", ".foo{tab-size:4px}");
    minify_test(".foo { -moz-tab-size: 4px }", ".foo{-moz-tab-size:4px}");
    minify_test(".foo { -o-tab-size: 4px }", ".foo{-o-tab-size:4px}");
    prefix_test(
      ".foo{ tab-size: 4 }",
      indoc! {r#"
      .foo {
        -moz-tab-size: 4;
        -o-tab-size: 4;
        tab-size: 4;
      }
      "#},
      Browsers {
        safari: Some(8 << 16),
        firefox: Some(50 << 16),
        opera: Some(12 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -moz-tab-size: 4;
        -o-tab-size: 4;
        tab-size: 4;
      }
      "#,
      indoc! {r#"
      .foo {
        tab-size: 4;
      }
      "#},
      Browsers {
        safari: Some(8 << 16),
        firefox: Some(94 << 16),
        opera: Some(30 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_word_break() {
    minify_test(".foo { word-break: normal }", ".foo{word-break:normal}");
    minify_test(".foo { word-break: keep-all }", ".foo{word-break:keep-all}");
    minify_test(".foo { word-break: break-all }", ".foo{word-break:break-all}");
    minify_test(".foo { word-break: break-word }", ".foo{word-break:break-word}");
  }

  #[test]
  fn test_line_break() {
    minify_test(".foo { line-break: auto }", ".foo{line-break:auto}");
    minify_test(".foo { line-break: Loose }", ".foo{line-break:loose}");
    minify_test(".foo { line-break: anywhere }", ".foo{line-break:anywhere}");
  }

  #[test]
  fn test_wrap() {
    minify_test(".foo { overflow-wrap: nOrmal }", ".foo{overflow-wrap:normal}");
    minify_test(".foo { overflow-wrap: break-Word }", ".foo{overflow-wrap:break-word}");
    minify_test(".foo { overflow-wrap: Anywhere }", ".foo{overflow-wrap:anywhere}");
    minify_test(".foo { word-wrap: Normal }", ".foo{word-wrap:normal}");
    minify_test(".foo { word-wrap: Break-wOrd }", ".foo{word-wrap:break-word}");
    minify_test(".foo { word-wrap: Anywhere }", ".foo{word-wrap:anywhere}");
  }

  #[test]
  fn test_hyphens() {
    minify_test(".foo { hyphens: manual }", ".foo{hyphens:manual}");
    minify_test(".foo { hyphens: auto }", ".foo{hyphens:auto}");
    minify_test(".foo { hyphens: none }", ".foo{hyphens:none}");
    minify_test(".foo { -webkit-hyphens: manual }", ".foo{-webkit-hyphens:manual}");
    minify_test(".foo { -moz-hyphens: manual }", ".foo{-moz-hyphens:manual}");
    minify_test(".foo { -ms-hyphens: manual }", ".foo{-ms-hyphens:manual}");
    prefix_test(
      ".foo{ hyphens: manual }",
      indoc! {r#"
      .foo {
        -webkit-hyphens: manual;
        -moz-hyphens: manual;
        -ms-hyphens: manual;
        hyphens: manual;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        firefox: Some(40 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-hyphens: manual;
        -moz-hyphens: manual;
        -ms-hyphens: manual;
        hyphens: manual;
      }
      "#,
      indoc! {r#"
      .foo {
        -webkit-hyphens: manual;
        hyphens: manual;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        chrome: Some(88 << 16),
        firefox: Some(88 << 16),
        edge: Some(79 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-hyphens: manual;
        -moz-hyphens: manual;
        -ms-hyphens: manual;
        hyphens: manual;
      }
      "#,
      indoc! {r#"
      .foo {
        hyphens: manual;
      }
      "#},
      Browsers {
        chrome: Some(88 << 16),
        firefox: Some(88 << 16),
        edge: Some(79 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_text_align() {
    minify_test(".foo { text-align: left }", ".foo{text-align:left}");
    minify_test(".foo { text-align: Left }", ".foo{text-align:left}");
    minify_test(".foo { text-align: END }", ".foo{text-align:end}");
    minify_test(".foo { text-align: left }", ".foo{text-align:left}");

    prefix_test(
      r#"
      .foo {
        text-align: start;
      }
    "#,
      indoc! {r#"
      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        text-align: left;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        text-align: right;
      }
    "#
      },
      Browsers {
        safari: Some(2 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-align: end;
      }
    "#,
      indoc! {r#"
      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        text-align: right;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        text-align: left;
      }
    "#
      },
      Browsers {
        safari: Some(2 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-align: start;
      }
    "#,
      indoc! {r#"
      .foo {
        text-align: start;
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo > .bar {
        text-align: start;
      }
    "#,
      indoc! {r#"
      .foo > .bar:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        text-align: left;
      }

      .foo > .bar:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        text-align: right;
      }
    "#
      },
      Browsers {
        safari: Some(2 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:after {
        text-align: start;
      }
    "#,
      indoc! {r#"
      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))):after {
        text-align: left;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)):after {
        text-align: right;
      }
    "#
      },
      Browsers {
        safari: Some(2 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo:hover {
        text-align: start;
      }
    "#,
      indoc! {r#"
      .foo:hover:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        text-align: left;
      }

      .foo:hover:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        text-align: right;
      }
    "#
      },
      Browsers {
        safari: Some(2 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_text_align_last() {
    minify_test(".foo { text-align-last: left }", ".foo{text-align-last:left}");
    minify_test(".foo { text-align-last: justify }", ".foo{text-align-last:justify}");
    prefix_test(
      ".foo{ text-align-last: left }",
      indoc! {r#"
      .foo {
        -moz-text-align-last: left;
        text-align-last: left;
      }
      "#},
      Browsers {
        firefox: Some(40 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -moz-text-align-last: left;
        text-align-last: left;
      }
      "#,
      indoc! {r#"
      .foo {
        text-align-last: left;
      }
      "#},
      Browsers {
        firefox: Some(88 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_text_justify() {
    minify_test(".foo { text-justify: auto }", ".foo{text-justify:auto}");
    minify_test(".foo { text-justify: inter-word }", ".foo{text-justify:inter-word}");
  }

  #[test]
  fn test_word_spacing() {
    minify_test(".foo { word-spacing: normal }", ".foo{word-spacing:normal}");
    minify_test(".foo { word-spacing: 3px }", ".foo{word-spacing:3px}");
  }

  #[test]
  fn test_letter_spacing() {
    minify_test(".foo { letter-spacing: normal }", ".foo{letter-spacing:normal}");
    minify_test(".foo { letter-spacing: 3px }", ".foo{letter-spacing:3px}");
  }

  #[test]
  fn test_text_indent() {
    minify_test(".foo { text-indent: 20px }", ".foo{text-indent:20px}");
    minify_test(".foo { text-indent: 10% }", ".foo{text-indent:10%}");
    minify_test(".foo { text-indent: 3em hanging }", ".foo{text-indent:3em hanging}");
    minify_test(".foo { text-indent: 3em each-line }", ".foo{text-indent:3em each-line}");
    minify_test(
      ".foo { text-indent: 3em hanging each-line }",
      ".foo{text-indent:3em hanging each-line}",
    );
    minify_test(
      ".foo { text-indent: 3em each-line hanging }",
      ".foo{text-indent:3em hanging each-line}",
    );
    minify_test(
      ".foo { text-indent: each-line 3em hanging }",
      ".foo{text-indent:3em hanging each-line}",
    );
    minify_test(
      ".foo { text-indent: each-line hanging 3em }",
      ".foo{text-indent:3em hanging each-line}",
    );
  }

  #[test]
  fn test_text_size_adjust() {
    minify_test(".foo { text-size-adjust: none }", ".foo{text-size-adjust:none}");
    minify_test(".foo { text-size-adjust: auto }", ".foo{text-size-adjust:auto}");
    minify_test(".foo { text-size-adjust: 80% }", ".foo{text-size-adjust:80%}");
    prefix_test(
      r#"
      .foo {
        text-size-adjust: none;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-size-adjust: none;
        -moz-text-size-adjust: none;
        -ms-text-size-adjust: none;
        text-size-adjust: none;
      }
    "#},
      Browsers {
        ios_saf: Some(16 << 16),
        edge: Some(15 << 16),
        firefox: Some(20 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-text-size-adjust: none;
        -moz-text-size-adjust: none;
        -ms-text-size-adjust: none;
        text-size-adjust: none;
      }
    "#,
      indoc! {r#"
      .foo {
        text-size-adjust: none;
      }
    "#},
      Browsers {
        chrome: Some(110 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_text_decoration() {
    minify_test(".foo { text-decoration-line: none }", ".foo{text-decoration-line:none}");
    minify_test(
      ".foo { text-decoration-line: underline }",
      ".foo{text-decoration-line:underline}",
    );
    minify_test(
      ".foo { text-decoration-line: overline }",
      ".foo{text-decoration-line:overline}",
    );
    minify_test(
      ".foo { text-decoration-line: line-through }",
      ".foo{text-decoration-line:line-through}",
    );
    minify_test(
      ".foo { text-decoration-line: blink }",
      ".foo{text-decoration-line:blink}",
    );
    minify_test(
      ".foo { text-decoration-line: underline overline }",
      ".foo{text-decoration-line:underline overline}",
    );
    minify_test(
      ".foo { text-decoration-line: overline underline }",
      ".foo{text-decoration-line:underline overline}",
    );
    minify_test(
      ".foo { text-decoration-line: overline line-through underline }",
      ".foo{text-decoration-line:underline overline line-through}",
    );
    minify_test(
      ".foo { text-decoration-line: spelling-error }",
      ".foo{text-decoration-line:spelling-error}",
    );
    minify_test(
      ".foo { text-decoration-line: grammar-error }",
      ".foo{text-decoration-line:grammar-error}",
    );
    minify_test(
      ".foo { -webkit-text-decoration-line: overline underline }",
      ".foo{-webkit-text-decoration-line:underline overline}",
    );
    minify_test(
      ".foo { -moz-text-decoration-line: overline underline }",
      ".foo{-moz-text-decoration-line:underline overline}",
    );

    minify_test(
      ".foo { text-decoration-style: solid }",
      ".foo{text-decoration-style:solid}",
    );
    minify_test(
      ".foo { text-decoration-style: dotted }",
      ".foo{text-decoration-style:dotted}",
    );
    minify_test(
      ".foo { -webkit-text-decoration-style: solid }",
      ".foo{-webkit-text-decoration-style:solid}",
    );

    minify_test(
      ".foo { text-decoration-color: yellow }",
      ".foo{text-decoration-color:#ff0}",
    );
    minify_test(
      ".foo { -webkit-text-decoration-color: yellow }",
      ".foo{-webkit-text-decoration-color:#ff0}",
    );

    minify_test(".foo { text-decoration: none }", ".foo{text-decoration:none}");
    minify_test(
      ".foo { text-decoration: underline dotted }",
      ".foo{text-decoration:underline dotted}",
    );
    minify_test(
      ".foo { text-decoration: underline dotted yellow }",
      ".foo{text-decoration:underline dotted #ff0}",
    );
    minify_test(
      ".foo { text-decoration: yellow dotted underline }",
      ".foo{text-decoration:underline dotted #ff0}",
    );
    minify_test(
      ".foo { text-decoration: underline overline dotted yellow }",
      ".foo{text-decoration:underline overline dotted #ff0}",
    );
    minify_test(
      ".foo { -webkit-text-decoration: yellow dotted underline }",
      ".foo{-webkit-text-decoration:underline dotted #ff0}",
    );
    minify_test(
      ".foo { -moz-text-decoration: yellow dotted underline }",
      ".foo{-moz-text-decoration:underline dotted #ff0}",
    );

    test(
      r#"
      .foo {
        text-decoration-line: underline;
        text-decoration-style: dotted;
        text-decoration-color: yellow;
        text-decoration-thickness: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration: underline 2px dotted #ff0;
      }
    "#},
    );

    test(
      r#"
      .foo {
        text-decoration: underline;
        text-decoration-style: dotted;
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration: underline dotted;
      }
    "#},
    );

    test(
      r#"
      .foo {
        text-decoration: underline;
        text-decoration-style: var(--style);
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration: underline;
        text-decoration-style: var(--style);
      }
    "#},
    );

    test(
      r#"
      .foo {
        -webkit-text-decoration: underline;
        -webkit-text-decoration-style: dotted;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration: underline dotted;
      }
    "#},
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: underline dotted;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration: underline dotted;
        text-decoration: underline dotted;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        firefox: Some(30 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration-line: underline;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration-line: underline;
        -moz-text-decoration-line: underline;
        text-decoration-line: underline;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        firefox: Some(30 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration-style: dotted;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration-style: dotted;
        -moz-text-decoration-style: dotted;
        text-decoration-style: dotted;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        firefox: Some(30 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration-color: yellow;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration-color: #ff0;
        -moz-text-decoration-color: #ff0;
        text-decoration-color: #ff0;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        firefox: Some(30 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: underline;
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration: underline;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        firefox: Some(30 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-text-decoration: underline dotted;
        text-decoration: underline dotted;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration: underline dotted;
        text-decoration: underline dotted;
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        firefox: Some(45 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: double underline;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration: underline double;
        text-decoration: underline double;
      }
    "#},
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: underline;
        text-decoration-style: double;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration: underline double;
        text-decoration: underline double;
      }
    "#},
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: underline;
        text-decoration-color: red;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration: underline red;
        text-decoration: underline red;
      }
    "#},
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: var(--test);
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration: var(--test);
        text-decoration: var(--test);
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        firefox: Some(30 << 16),
        ..Browsers::default()
      },
    );

    minify_test(
      ".foo { text-decoration-skip-ink: all }",
      ".foo{text-decoration-skip-ink:all}",
    );
    minify_test(
      ".foo { -webkit-text-decoration-skip-ink: all }",
      ".foo{-webkit-text-decoration-skip-ink:all}",
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: lch(50.998% 135.363 338) underline;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration: underline #ee00be;
        text-decoration: underline #ee00be;
        -webkit-text-decoration: underline lch(50.998% 135.363 338);
        text-decoration: underline lch(50.998% 135.363 338);
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        firefox: Some(30 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration-color: lch(50.998% 135.363 338);
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-decoration-color: #ee00be;
        -moz-text-decoration-color: #ee00be;
        text-decoration-color: #ee00be;
        -webkit-text-decoration-color: lch(50.998% 135.363 338);
        -moz-text-decoration-color: lch(50.998% 135.363 338);
        text-decoration-color: lch(50.998% 135.363 338);
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        firefox: Some(30 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: lch(50.998% 135.363 338) var(--style);
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration: #ee00be var(--style);
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          text-decoration: lab(50.998% 125.506 -50.7078) var(--style);
        }
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: underline 10px;
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration: underline;
        text-decoration-thickness: 10px;
      }
    "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: underline 10px;
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration: underline 10px;
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: underline 10%;
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration: underline;
        text-decoration-thickness: calc(1em / 10);
      }
    "#},
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: underline 10%;
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration: underline 10%;
      }
    "#},
      Browsers {
        firefox: Some(89 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration-thickness: 10%;
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration-thickness: calc(1em / 10);
      }
    "#},
      Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration-thickness: 10%;
      }
    "#,
      indoc! {r#"
      .foo {
        text-decoration-thickness: 10%;
      }
    "#},
      Browsers {
        firefox: Some(89 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_text_emphasis() {
    minify_test(".foo { text-emphasis-style: none }", ".foo{text-emphasis-style:none}");
    minify_test(
      ".foo { text-emphasis-style: filled }",
      ".foo{text-emphasis-style:filled}",
    );
    minify_test(".foo { text-emphasis-style: open }", ".foo{text-emphasis-style:open}");
    minify_test(".foo { text-emphasis-style: dot }", ".foo{text-emphasis-style:dot}");
    minify_test(
      ".foo { text-emphasis-style: filled dot }",
      ".foo{text-emphasis-style:dot}",
    );
    minify_test(
      ".foo { text-emphasis-style: dot filled }",
      ".foo{text-emphasis-style:dot}",
    );
    minify_test(
      ".foo { text-emphasis-style: open dot }",
      ".foo{text-emphasis-style:open dot}",
    );
    minify_test(
      ".foo { text-emphasis-style: dot open }",
      ".foo{text-emphasis-style:open dot}",
    );
    minify_test(".foo { text-emphasis-style: \"x\" }", ".foo{text-emphasis-style:\"x\"}");

    minify_test(".foo { text-emphasis-color: yellow }", ".foo{text-emphasis-color:#ff0}");

    minify_test(".foo { text-emphasis: none }", ".foo{text-emphasis:none}");
    minify_test(".foo { text-emphasis: filled }", ".foo{text-emphasis:filled}");
    minify_test(
      ".foo { text-emphasis: filled yellow }",
      ".foo{text-emphasis:filled #ff0}",
    );
    minify_test(
      ".foo { text-emphasis: dot filled yellow }",
      ".foo{text-emphasis:dot #ff0}",
    );

    test(
      r#"
      .foo {
        text-emphasis-style: filled;
        text-emphasis-color: yellow;
      }
    "#,
      indoc! {r#"
      .foo {
        text-emphasis: filled #ff0;
      }
    "#},
    );

    test(
      r#"
      .foo {
        text-emphasis: filled red;
        text-emphasis-color: yellow;
      }
    "#,
      indoc! {r#"
      .foo {
        text-emphasis: filled #ff0;
      }
    "#},
    );

    test(
      r#"
      .foo {
        text-emphasis: filled yellow;
        text-emphasis-color: var(--color);
      }
    "#,
      indoc! {r#"
      .foo {
        text-emphasis: filled #ff0;
        text-emphasis-color: var(--color);
      }
    "#},
    );

    prefix_test(
      r#"
      .foo {
        text-emphasis-style: filled;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-emphasis-style: filled;
        text-emphasis-style: filled;
      }
    "#},
      Browsers {
        safari: Some(10 << 16),
        chrome: Some(30 << 16),
        firefox: Some(45 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-text-emphasis-style: filled;
        text-emphasis-style: filled;
      }
    "#,
      indoc! {r#"
      .foo {
        text-emphasis-style: filled;
      }
    "#},
      Browsers {
        safari: Some(10 << 16),
        firefox: Some(45 << 16),
        ..Browsers::default()
      },
    );

    minify_test(
      ".foo { text-emphasis-position: over }",
      ".foo{text-emphasis-position:over}",
    );
    minify_test(
      ".foo { text-emphasis-position: under }",
      ".foo{text-emphasis-position:under}",
    );
    minify_test(
      ".foo { text-emphasis-position: over right }",
      ".foo{text-emphasis-position:over}",
    );
    minify_test(
      ".foo { text-emphasis-position: over left }",
      ".foo{text-emphasis-position:over left}",
    );

    prefix_test(
      r#"
      .foo {
        text-emphasis-position: over;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-emphasis-position: over;
        text-emphasis-position: over;
      }
    "#},
      Browsers {
        safari: Some(10 << 16),
        chrome: Some(30 << 16),
        firefox: Some(45 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-emphasis-position: over left;
      }
    "#,
      indoc! {r#"
      .foo {
        text-emphasis-position: over left;
      }
    "#},
      Browsers {
        safari: Some(10 << 16),
        chrome: Some(30 << 16),
        firefox: Some(45 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-emphasis-position: var(--test);
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-emphasis-position: var(--test);
        text-emphasis-position: var(--test);
      }
    "#},
      Browsers {
        safari: Some(10 << 16),
        chrome: Some(30 << 16),
        firefox: Some(45 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-emphasis: filled lch(50.998% 135.363 338);
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-emphasis: filled #ee00be;
        text-emphasis: filled #ee00be;
        -webkit-text-emphasis: filled lch(50.998% 135.363 338);
        text-emphasis: filled lch(50.998% 135.363 338);
      }
    "#},
      Browsers {
        chrome: Some(25 << 16),
        firefox: Some(48 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-emphasis-color: lch(50.998% 135.363 338);
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-text-emphasis-color: #ee00be;
        text-emphasis-color: #ee00be;
        -webkit-text-emphasis-color: lch(50.998% 135.363 338);
        text-emphasis-color: lch(50.998% 135.363 338);
      }
    "#},
      Browsers {
        chrome: Some(25 << 16),
        firefox: Some(48 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-emphasis: lch(50.998% 135.363 338) var(--style);
      }
    "#,
      indoc! {r#"
      .foo {
        text-emphasis: #ee00be var(--style);
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          text-emphasis: lab(50.998% 125.506 -50.7078) var(--style);
        }
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_text_shadow() {
    minify_test(
      ".foo { text-shadow: 1px 1px 2px yellow; }",
      ".foo{text-shadow:1px 1px 2px #ff0}",
    );
    minify_test(
      ".foo { text-shadow: 1px 1px 2px 3px yellow; }",
      ".foo{text-shadow:1px 1px 2px 3px #ff0}",
    );
    minify_test(
      ".foo { text-shadow: 1px 1px 0 yellow; }",
      ".foo{text-shadow:1px 1px #ff0}",
    );
    minify_test(
      ".foo { text-shadow: 1px 1px yellow; }",
      ".foo{text-shadow:1px 1px #ff0}",
    );
    minify_test(
      ".foo { text-shadow: 1px 1px yellow, 2px 3px red; }",
      ".foo{text-shadow:1px 1px #ff0,2px 3px red}",
    );

    prefix_test(
      ".foo { text-shadow: 12px 12px lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          text-shadow: 12px 12px #b32323;
          text-shadow: 12px 12px lab(40% 56.6 39);
        }
      "#},
      Browsers {
        chrome: Some(4 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { text-shadow: 12px 12px lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          text-shadow: 12px 12px #b32323;
          text-shadow: 12px 12px color(display-p3 .643308 .192455 .167712);
          text-shadow: 12px 12px lab(40% 56.6 39);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { text-shadow: 12px 12px lab(40% 56.6 39), 12px 12px yellow }",
      indoc! { r#"
        .foo {
          text-shadow: 12px 12px #b32323, 12px 12px #ff0;
          text-shadow: 12px 12px lab(40% 56.6 39), 12px 12px #ff0;
        }
      "#},
      Browsers {
        chrome: Some(4 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { text-shadow: var(--foo) 12px lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          text-shadow: var(--foo) 12px #b32323;
        }

        @supports (color: lab(0% 0 0)) {
          .foo {
            text-shadow: var(--foo) 12px lab(40% 56.6 39);
          }
        }
      "#},
      Browsers {
        chrome: Some(4 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_break() {
    prefix_test(
      r#"
      .foo {
        box-decoration-break: clone;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-box-decoration-break: clone;
        box-decoration-break: clone;
      }
    "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        box-decoration-break: clone;
      }
    "#,
      indoc! {r#"
      .foo {
        box-decoration-break: clone;
      }
    "#},
      Browsers {
        firefox: Some(95 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_position() {
    test(
      r#"
      .foo {
        position: relative;
        position: absolute;
      }
    "#,
      indoc! {r#"
      .foo {
        position: absolute;
      }
    "#},
    );

    test(
      r#"
      .foo {
        position: -webkit-sticky;
        position: sticky;
      }
    "#,
      indoc! {r#"
      .foo {
        position: -webkit-sticky;
        position: sticky;
      }
    "#},
    );

    prefix_test(
      r#"
      .foo {
        position: sticky;
      }
    "#,
      indoc! {r#"
      .foo {
        position: -webkit-sticky;
        position: sticky;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        position: -webkit-sticky;
        position: sticky;
      }
    "#,
      indoc! {r#"
      .foo {
        position: sticky;
      }
    "#},
      Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
      .foo {
        top: 0;
        left: 0;
        bottom: 0;
        right: 0;
      }
    "#,
      indoc! {r#"
      .foo {
        inset: 0;
      }
    "#},
    );

    test(
      r#"
      .foo {
        top: 2px;
        left: 4px;
        bottom: 2px;
        right: 4px;
      }
    "#,
      indoc! {r#"
      .foo {
        inset: 2px 4px;
      }
    "#},
    );

    test(
      r#"
      .foo {
        top: 1px;
        left: 2px;
        bottom: 3px;
        right: 4px;
      }
    "#,
      indoc! {r#"
      .foo {
        inset: 1px 4px 3px 2px;
      }
    "#},
    );

    test(
      r#"
      .foo {
        inset-block-start: 2px;
        inset-block-end: 2px;
        inset-inline-start: 4px;
        inset-inline-end: 4px;
      }
    "#,
      indoc! {r#"
      .foo {
        inset-block: 2px;
        inset-inline: 4px;
      }
    "#},
    );

    test(
      r#"
      .foo {
        inset-block-start: 2px;
        inset-block-end: 3px;
        inset-inline-start: 4px;
        inset-inline-end: 5px;
      }
    "#,
      indoc! {r#"
      .foo {
        inset-block: 2px 3px;
        inset-inline: 4px 5px;
      }
    "#},
    );

    test(
      r#"
      .foo {
        inset-block-start: 2px;
        inset-block-end: 3px;
        inset: 4px;
        inset-inline-start: 4px;
        inset-inline-end: 5px;
      }
    "#,
      indoc! {r#"
      .foo {
        inset: 4px;
        inset-inline: 4px 5px;
      }
    "#},
    );

    prefix_test(
      r#"
      .foo {
        inset-inline-start: 2px;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        left: 2px;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        left: 2px;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        right: 2px;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        right: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        inset-inline-start: 2px;
        inset-inline-end: 4px;
      }
    "#,
      indoc! {r#"
      .foo:not(:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        left: 2px;
        right: 4px;
      }

      .foo:not(:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi))) {
        left: 2px;
        right: 4px;
      }

      .foo:-webkit-any(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        left: 4px;
        right: 2px;
      }

      .foo:is(:lang(ae), :lang(ar), :lang(arc), :lang(bcc), :lang(bqi), :lang(ckb), :lang(dv), :lang(fa), :lang(glk), :lang(he), :lang(ku), :lang(mzn), :lang(nqo), :lang(pnb), :lang(ps), :lang(sd), :lang(ug), :lang(ur), :lang(yi)) {
        left: 4px;
        right: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        inset-inline: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        left: 2px;
        right: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        inset-block-start: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        top: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        inset-block-end: 2px;
      }
    "#,
      indoc! {r#"
      .foo {
        bottom: 2px;
      }
    "#
      },
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        top: 1px;
        left: 2px;
        bottom: 3px;
        right: 4px;
      }
    "#,
      indoc! {r#"
      .foo {
        top: 1px;
        bottom: 3px;
        left: 2px;
        right: 4px;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_overflow() {
    minify_test(".foo { overflow: hidden }", ".foo{overflow:hidden}");
    minify_test(".foo { overflow: hidden hidden }", ".foo{overflow:hidden}");
    minify_test(".foo { overflow: hidden auto }", ".foo{overflow:hidden auto}");

    test(
      r#"
      .foo {
        overflow-x: hidden;
        overflow-y: auto;
      }
    "#,
      indoc! {r#"
      .foo {
        overflow: hidden auto;
      }
    "#},
    );

    test(
      r#"
      .foo {
        overflow: hidden;
        overflow-y: auto;
      }
    "#,
      indoc! {r#"
      .foo {
        overflow: hidden auto;
      }
    "#},
    );
    test(
      r#"
      .foo {
        overflow: hidden;
        overflow-y: var(--y);
      }
    "#,
      indoc! {r#"
      .foo {
        overflow: hidden;
        overflow-y: var(--y);
      }
    "#},
    );
    prefix_test(
      r#"
      .foo {
        overflow: hidden auto;
      }
    "#,
      indoc! {r#"
      .foo {
        overflow-x: hidden;
        overflow-y: auto;
      }
    "#},
      Browsers {
        chrome: Some(67 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        overflow: hidden hidden;
      }
    "#,
      indoc! {r#"
      .foo {
        overflow: hidden;
      }
    "#},
      Browsers {
        chrome: Some(67 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        overflow: hidden auto;
      }
    "#,
      indoc! {r#"
      .foo {
        overflow: hidden auto;
      }
    "#},
      Browsers {
        chrome: Some(68 << 16),
        ..Browsers::default()
      },
    );

    minify_test(".foo { text-overflow: ellipsis }", ".foo{text-overflow:ellipsis}");
    prefix_test(
      r#"
      .foo {
        text-overflow: ellipsis;
      }
    "#,
      indoc! {r#"
      .foo {
        -o-text-overflow: ellipsis;
        text-overflow: ellipsis;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        opera: Some(10 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -o-text-overflow: ellipsis;
        text-overflow: ellipsis;
      }
    "#,
      indoc! {r#"
      .foo {
        text-overflow: ellipsis;
      }
    "#},
      Browsers {
        safari: Some(4 << 16),
        opera: Some(14 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_ui() {
    minify_test(".foo { resize: both }", ".foo{resize:both}");
    minify_test(".foo { resize: Horizontal }", ".foo{resize:horizontal}");
    minify_test(".foo { cursor: ew-resize }", ".foo{cursor:ew-resize}");
    minify_test(
      ".foo { cursor: url(\"test.cur\"), ew-resize }",
      ".foo{cursor:url(test.cur),ew-resize}",
    );
    minify_test(
      ".foo { cursor: url(\"test.cur\"), url(\"foo.cur\"), ew-resize }",
      ".foo{cursor:url(test.cur),url(foo.cur),ew-resize}",
    );
    minify_test(".foo { caret-color: auto }", ".foo{caret-color:auto}");
    minify_test(".foo { caret-color: yellow }", ".foo{caret-color:#ff0}");
    minify_test(".foo { caret-shape: block }", ".foo{caret-shape:block}");
    minify_test(".foo { caret: yellow block }", ".foo{caret:#ff0 block}");
    minify_test(".foo { caret: block yellow }", ".foo{caret:#ff0 block}");
    minify_test(".foo { caret: block }", ".foo{caret:block}");
    minify_test(".foo { caret: yellow }", ".foo{caret:#ff0}");
    minify_test(".foo { caret: auto auto }", ".foo{caret:auto}");
    minify_test(".foo { caret: auto }", ".foo{caret:auto}");
    minify_test(".foo { caret: yellow auto }", ".foo{caret:#ff0}");
    minify_test(".foo { caret: auto block }", ".foo{caret:block}");
    minify_test(".foo { user-select: none }", ".foo{user-select:none}");
    minify_test(".foo { -webkit-user-select: none }", ".foo{-webkit-user-select:none}");
    minify_test(".foo { accent-color: auto }", ".foo{accent-color:auto}");
    minify_test(".foo { accent-color: yellow }", ".foo{accent-color:#ff0}");
    minify_test(".foo { appearance: None }", ".foo{appearance:none}");
    minify_test(
      ".foo { -webkit-appearance: textfield }",
      ".foo{-webkit-appearance:textfield}",
    );

    prefix_test(
      r#"
      .foo {
        user-select: none;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-user-select: none;
        -moz-user-select: none;
        -ms-user-select: none;
        user-select: none;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        opera: Some(5 << 16),
        firefox: Some(10 << 16),
        ie: Some(10 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-user-select: none;
        -moz-user-select: none;
        -ms-user-select: none;
        user-select: none;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-user-select: none;
        user-select: none;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        opera: Some(80 << 16),
        firefox: Some(80 << 16),
        edge: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-user-select: none;
        -moz-user-select: none;
        -ms-user-select: none;
        user-select: none;
      }
    "#,
      indoc! {r#"
      .foo {
        user-select: none;
      }
    "#},
      Browsers {
        opera: Some(80 << 16),
        firefox: Some(80 << 16),
        edge: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        appearance: none;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-appearance: none;
        -moz-appearance: none;
        -ms-appearance: none;
        appearance: none;
      }
    "#},
      Browsers {
        safari: Some(8 << 16),
        chrome: Some(80 << 16),
        firefox: Some(10 << 16),
        ie: Some(11 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-appearance: none;
        -moz-appearance: none;
        -ms-appearance: none;
        appearance: none;
      }
    "#,
      indoc! {r#"
      .foo {
        -webkit-appearance: none;
        appearance: none;
      }
    "#},
      Browsers {
        safari: Some(15 << 16),
        chrome: Some(85 << 16),
        firefox: Some(80 << 16),
        edge: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        -webkit-appearance: none;
        -moz-appearance: none;
        -ms-appearance: none;
        appearance: none;
      }
    "#,
      indoc! {r#"
      .foo {
        appearance: none;
      }
    "#},
      Browsers {
        chrome: Some(85 << 16),
        firefox: Some(80 << 16),
        edge: Some(85 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { caret-color: lch(50.998% 135.363 338) }",
      indoc! { r#"
        .foo {
          caret-color: #ee00be;
          caret-color: color(display-p3 .972962 -.362078 .804206);
          caret-color: lch(50.998% 135.363 338);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { caret: lch(50.998% 135.363 338) block }",
      indoc! { r#"
        .foo {
          caret: #ee00be block;
          caret: color(display-p3 .972962 -.362078 .804206) block;
          caret: lch(50.998% 135.363 338) block;
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { caret: lch(50.998% 135.363 338) var(--foo) }",
      indoc! { r#"
        .foo {
          caret: #ee00be var(--foo);
        }

        @supports (color: lab(0% 0 0)) {
          .foo {
            caret: lab(50.998% 125.506 -50.7078) var(--foo);
          }
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_list() {
    minify_test(".foo { list-style-type: disc; }", ".foo{list-style-type:disc}");
    minify_test(".foo { list-style-type: \"★\"; }", ".foo{list-style-type:\"★\"}");
    minify_test(
      ".foo { list-style-type: symbols(cyclic '○' '●'); }",
      ".foo{list-style-type:symbols(cyclic \"○\" \"●\")}",
    );
    minify_test(
      ".foo { list-style-type: symbols('○' '●'); }",
      ".foo{list-style-type:symbols(\"○\" \"●\")}",
    );
    minify_test(
      ".foo { list-style-type: symbols(symbolic '○' '●'); }",
      ".foo{list-style-type:symbols(\"○\" \"●\")}",
    );
    minify_test(
      ".foo { list-style-type: symbols(symbolic url('ellipse.png')); }",
      ".foo{list-style-type:symbols(url(ellipse.png))}",
    );
    minify_test(
      ".foo { list-style-image: url('ellipse.png'); }",
      ".foo{list-style-image:url(ellipse.png)}",
    );
    minify_test(
      ".foo { list-style-position: outside; }",
      ".foo{list-style-position:outside}",
    );
    minify_test(
      ".foo { list-style: \"★\" url(ellipse.png) outside; }",
      ".foo{list-style:url(ellipse.png) \"★\"}",
    );
    minify_test(".foo { list-style: none; }", ".foo{list-style:none}");
    minify_test(".foo { list-style: none none outside; }", ".foo{list-style:none}");
    minify_test(".foo { list-style: none none inside; }", ".foo{list-style:inside none}");
    minify_test(".foo { list-style: none inside; }", ".foo{list-style:inside none}");
    minify_test(".foo { list-style: none disc; }", ".foo{list-style:outside}");
    minify_test(".foo { list-style: none inside disc; }", ".foo{list-style:inside}");
    minify_test(".foo { list-style: none \"★\"; }", ".foo{list-style:\"★\"}");
    minify_test(
      ".foo { list-style: none url(foo.png); }",
      ".foo{list-style:url(foo.png) none}",
    );

    test(
      r#"
      .foo {
        list-style-type: disc;
        list-style-image: url(ellipse.png);
        list-style-position: outside;
      }
    "#,
      indoc! {r#"
      .foo {
        list-style: url("ellipse.png");
      }
    "#},
    );

    test(
      r#"
      .foo {
        list-style: \"★\" url(ellipse.png) outside;
        list-style-image: none;
      }
    "#,
      indoc! {r#"
      .foo {
        list-style: \"★\";
      }
    "#},
    );

    test(
      r#"
      .foo {
        list-style: \"★\" url(ellipse.png) outside;
        list-style-image: var(--img);
      }
    "#,
      indoc! {r#"
      .foo {
        list-style: url("ellipse.png") \"★\";
        list-style-image: var(--img);
      }
    "#},
    );

    prefix_test(
      ".foo { list-style-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          list-style-image: -webkit-gradient(linear, 0 0, 0 100%, from(#ff0f0e), to(#7773ff));
          list-style-image: -webkit-linear-gradient(#ff0f0e, #7773ff);
          list-style-image: linear-gradient(#ff0f0e, #7773ff);
          list-style-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { list-style: \"★\" linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          list-style: linear-gradient(#ff0f0e, #7773ff) "★";
          list-style: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) "★";
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { list-style: var(--foo) linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          list-style: var(--foo) linear-gradient(#ff0f0e, #7773ff);
        }

        @supports (color: lab(0% 0 0)) {
          .foo {
            list-style: var(--foo) linear-gradient(lab(56.208% 94.4644 98.8928), lab(51% 70.4544 -115.586));
          }
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
      .foo {
        list-style: inside;
        list-style-type: disc;
      }
    "#,
      indoc! {r#"
      .foo {
        list-style: inside;
      }
    "#},
    );
    test(
      r#"
      .foo {
        list-style: inside;
        list-style-type: decimal;
      }
    "#,
      indoc! {r#"
      .foo {
        list-style: inside decimal;
      }
    "#},
    );
  }

  #[test]
  fn test_image_set() {
    // Spec: https://drafts.csswg.org/css-images-4/#image-set-notation
    // WPT: https://github.com/web-platform-tests/wpt/blob/master/css/css-images/image-set/image-set-parsing.html
    // test image-set(<string>)
    minify_test(
      ".foo { background: image-set(\"foo.png\" 2x, url(bar.png) 1x) }",
      ".foo{background:image-set(\"foo.png\" 2x,\"bar.png\" 1x)}",
    );

    // test image-set(type(<string>))
    minify_test(
      ".foo { background: image-set('foo.webp' type('webp'), url(foo.jpg)) }",
      ".foo{background:image-set(\"foo.webp\" 1x type(\"webp\"),\"foo.jpg\" 1x)}",
    );
    minify_test(
      ".foo { background: image-set('foo.avif' 2x type('image/avif'), url(foo.png)) }",
      ".foo{background:image-set(\"foo.avif\" 2x type(\"image/avif\"),\"foo.png\" 1x)}",
    );
    minify_test(
      ".foo { background: image-set(url('example.png') 3x type('image/png')) }",
      ".foo{background:image-set(\"example.png\" 3x type(\"image/png\"))}",
    );

    minify_test(
      ".foo { background: image-set(url(example.png) type('image/png') 1x) }",
      ".foo{background:image-set(\"example.png\" 1x type(\"image/png\"))}",
    );

    minify_test(
      ".foo { background: -webkit-image-set(url(\"foo.png\") 2x, url(bar.png) 1x) }",
      ".foo{background:-webkit-image-set(url(foo.png) 2x,url(bar.png) 1x)}",
    );

    test(
      r#"
      .foo {
        background: -webkit-image-set(url("foo.png") 2x, url(bar.png) 1x);
        background: image-set(url("foo.png") 2x, url(bar.png) 1x);
      }
    "#,
      indoc! {r#"
      .foo {
        background: -webkit-image-set(url("foo.png") 2x, url("bar.png") 1x);
        background: image-set("foo.png" 2x, "bar.png" 1x);
      }
    "#},
    );

    // test image-set(<gradient>)
    test(
      r#"
      .foo {
        background: image-set(linear-gradient(cornflowerblue, white) 1x, url("detailed-gradient.png") 3x);
      }
    "#,
      indoc! {r#"
      .foo {
        background: image-set(linear-gradient(#6495ed, #fff) 1x, "detailed-gradient.png" 3x);
      }
    "#},
    );

    prefix_test(
      r#"
      .foo {
        background: image-set(url("foo.png") 2x, url(bar.png) 1x);
      }
    "#,
      indoc! {r#"
      .foo {
        background: -webkit-image-set(url("foo.png") 2x, url("bar.png") 1x);
        background: image-set("foo.png" 2x, "bar.png" 1x);
      }
    "#},
      Browsers {
        chrome: Some(85 << 16),
        firefox: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background: -webkit-image-set(url("foo.png") 2x, url(bar.png) 1x);
        background: image-set(url("foo.png") 2x, url(bar.png) 1x);
      }
    "#,
      indoc! {r#"
      .foo {
        background: -webkit-image-set(url("foo.png") 2x, url("bar.png") 1x);
        background: image-set("foo.png" 2x, "bar.png" 1x);
      }
    "#},
      Browsers {
        firefox: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        background: -webkit-image-set(url("foo.png") 2x, url(bar.png) 1x);
      }
    "#,
      indoc! {r#"
      .foo {
        background: -webkit-image-set(url("foo.png") 2x, url("bar.png") 1x);
      }
    "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    for property in &[
      "background",
      "background-image",
      "border-image-source",
      "border-image",
      "border-image-source",
      "-webkit-mask-image",
      "-webkit-mask",
      "list-style-image",
      "list-style",
    ] {
      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: url(foo.png);
          {}: image-set(url("foo.png") 2x, url(bar.png) 1x);
        }}
      "#,
          property, property
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: url("foo.png");
          {}: image-set("foo.png" 2x, "bar.png" 1x);
        }}
      "#},
          property, property
        ),
        Browsers {
          ie: Some(11 << 16),
          chrome: Some(95 << 16),
          ..Browsers::default()
        },
      );

      prefix_test(
        &format!(
          r#"
        .foo {{
          {}: url(foo.png);
          {}: image-set(url("foo.png") 2x, url(bar.png) 1x);
        }}
      "#,
          property, property
        ),
        &format!(
          indoc! {r#"
        .foo {{
          {}: -webkit-image-set(url("foo.png") 2x, url("bar.png") 1x);
          {}: image-set("foo.png" 2x, "bar.png" 1x);
        }}
      "#},
          property, property
        ),
        Browsers {
          chrome: Some(95 << 16),
          ..Browsers::default()
        },
      );
    }
  }

  #[test]
  fn test_color() {
    minify_test(".foo { color: yellow }", ".foo{color:#ff0}");
    minify_test(".foo { color: rgb(255, 255, 0) }", ".foo{color:#ff0}");
    minify_test(".foo { color: rgba(255, 255, 0, 1) }", ".foo{color:#ff0}");
    minify_test(".foo { color: rgba(255, 255, 0, 0.8) }", ".foo{color:#ff0c}");
    minify_test(".foo { color: rgb(128, 128, 128) }", ".foo{color:gray}");
    minify_test(".foo { color: rgb(123, 255, 255) }", ".foo{color:#7bffff}");
    minify_test(".foo { color: rgba(123, 255, 255, 0.5) }", ".foo{color:#7bffff80}");
    minify_test(".foo { color: rgb(123 255 255) }", ".foo{color:#7bffff}");
    minify_test(".foo { color: rgb(123 255 255 / .5) }", ".foo{color:#7bffff80}");
    minify_test(".foo { color: rgb(123 255 255 / 50%) }", ".foo{color:#7bffff80}");
    minify_test(".foo { color: rgb(48% 100% 100% / 50%) }", ".foo{color:#7affff80}");
    minify_test(".foo { color: hsl(100deg, 100%, 50%) }", ".foo{color:#5f0}");
    minify_test(".foo { color: hsl(100, 100%, 50%) }", ".foo{color:#5f0}");
    minify_test(".foo { color: hsl(100 100% 50%) }", ".foo{color:#5f0}");
    minify_test(".foo { color: hsl(100, 100%, 50%, .8) }", ".foo{color:#5f0c}");
    minify_test(".foo { color: hsl(100 100% 50% / .8) }", ".foo{color:#5f0c}");
    minify_test(".foo { color: hsla(100, 100%, 50%, .8) }", ".foo{color:#5f0c}");
    minify_test(".foo { color: hsla(100 100% 50% / .8) }", ".foo{color:#5f0c}");
    minify_test(".foo { color: transparent }", ".foo{color:#0000}");
    minify_test(".foo { color: currentColor }", ".foo{color:currentColor}");
    minify_test(".foo { color: ButtonBorder }", ".foo{color:buttonborder}");
    minify_test(".foo { color: hwb(194 0% 0%) }", ".foo{color:#00c4ff}");
    minify_test(".foo { color: hwb(194 0% 0% / 50%) }", ".foo{color:#00c4ff80}");
    minify_test(".foo { color: hwb(194 0% 50%) }", ".foo{color:#006280}");
    minify_test(".foo { color: hwb(194 50% 0%) }", ".foo{color:#80e1ff}");
    minify_test(".foo { color: hwb(194 50% 50%) }", ".foo{color:gray}");
    // minify_test(".foo { color: ActiveText }", ".foo{color:ActiveTet}");
    minify_test(
      ".foo { color: lab(29.2345% 39.3825 20.0664); }",
      ".foo{color:lab(29.2345% 39.3825 20.0664)}",
    );
    minify_test(
      ".foo { color: lab(29.2345% 39.3825 20.0664 / 100%); }",
      ".foo{color:lab(29.2345% 39.3825 20.0664)}",
    );
    minify_test(
      ".foo { color: lab(29.2345% 39.3825 20.0664 / 50%); }",
      ".foo{color:lab(29.2345% 39.3825 20.0664/.5)}",
    );
    minify_test(
      ".foo { color: lch(29.2345% 44.2 27); }",
      ".foo{color:lch(29.2345% 44.2 27)}",
    );
    minify_test(
      ".foo { color: lch(29.2345% 44.2 45deg); }",
      ".foo{color:lch(29.2345% 44.2 45)}",
    );
    minify_test(
      ".foo { color: lch(29.2345% 44.2 .5turn); }",
      ".foo{color:lch(29.2345% 44.2 180)}",
    );
    minify_test(
      ".foo { color: lch(29.2345% 44.2 27 / 100%); }",
      ".foo{color:lch(29.2345% 44.2 27)}",
    );
    minify_test(
      ".foo { color: lch(29.2345% 44.2 27 / 50%); }",
      ".foo{color:lch(29.2345% 44.2 27/.5)}",
    );
    minify_test(
      ".foo { color: oklab(40.101% 0.1147 0.0453); }",
      ".foo{color:oklab(40.101% .1147 .0453)}",
    );
    minify_test(
      ".foo { color: oklch(40.101% 0.12332 21.555); }",
      ".foo{color:oklch(40.101% .12332 21.555)}",
    );
    minify_test(
      ".foo { color: oklch(40.101% 0.12332 .5turn); }",
      ".foo{color:oklch(40.101% .12332 180)}",
    );
    minify_test(
      ".foo { color: color(display-p3 1 0.5 0); }",
      ".foo{color:color(display-p3 1 .5 0)}",
    );
    minify_test(
      ".foo { color: color(display-p3 100% 50% 0%); }",
      ".foo{color:color(display-p3 1 .5 0)}",
    );
    minify_test(
      ".foo { color: color(xyz-d50 0.2005 0.14089 0.4472); }",
      ".foo{color:color(xyz-d50 .2005 .14089 .4472)}",
    );
    minify_test(
      ".foo { color: color(xyz-d50 20.05% 14.089% 44.72%); }",
      ".foo{color:color(xyz-d50 .2005 .14089 .4472)}",
    );
    minify_test(
      ".foo { color: color(xyz-d65 0.2005 0.14089 0.4472); }",
      ".foo{color:color(xyz .2005 .14089 .4472)}",
    );
    minify_test(
      ".foo { color: color(xyz-d65 20.05% 14.089% 44.72%); }",
      ".foo{color:color(xyz .2005 .14089 .4472)}",
    );
    minify_test(
      ".foo { color: color(xyz 0.2005 0.14089 0.4472); }",
      ".foo{color:color(xyz .2005 .14089 .4472)}",
    );
    minify_test(
      ".foo { color: color(xyz 20.05% 14.089% 44.72%); }",
      ".foo{color:color(xyz .2005 .14089 .4472)}",
    );
    minify_test(
      ".foo { color: color(xyz 0.2005 0 0); }",
      ".foo{color:color(xyz .2005 0 0)}",
    );
    minify_test(".foo { color: color(xyz 0 0 0); }", ".foo{color:color(xyz 0 0 0)}");
    minify_test(".foo { color: color(xyz 0 1 0); }", ".foo{color:color(xyz 0 1 0)}");
    minify_test(
      ".foo { color: color(xyz 0 1 0 / 20%); }",
      ".foo{color:color(xyz 0 1 0/.2)}",
    );
    minify_test(
      ".foo { color: color(xyz 0 0 0 / 20%); }",
      ".foo{color:color(xyz 0 0 0/.2)}",
    );
    minify_test(
      ".foo { color: color(display-p3 100% 50% 0 / 20%); }",
      ".foo{color:color(display-p3 1 .5 0/.2)}",
    );
    minify_test(
      ".foo { color: color(display-p3 100% 0 0 / 20%); }",
      ".foo{color:color(display-p3 1 0 0/.2)}",
    );
    minify_test(".foo { color: hsl(none none none) }", ".foo{color:#000}");
    minify_test(".foo { color: hwb(none none none) }", ".foo{color:red}");
    minify_test(".foo { color: rgb(none none none) }", ".foo{color:#000}");

    // If the browser doesn't support `#rrggbbaa` color syntax, it is converted to `transparent`.
    attr_test(
      "color: rgba(0, 0, 0, 0)",
      "color:transparent",
      true,
      Some(Browsers {
        chrome: Some(61 << 16), // Chrome >= 62 supports `#rrggbbaa` color.
        ..Browsers::default()
      }),
    );

    attr_test(
      "color: #0000",
      "color:transparent",
      true,
      Some(Browsers {
        chrome: Some(61 << 16), // Chrome >= 62 supports `#rrggbbaa` color.
        ..Browsers::default()
      }),
    );

    attr_test(
      "color: transparent",
      "color:transparent",
      true,
      Some(Browsers {
        chrome: Some(61 << 16),
        ..Browsers::default()
      }),
    );

    attr_test(
      "color: rgba(0, 0, 0, 0)",
      "color: rgba(0, 0, 0, 0)",
      false,
      Some(Browsers {
        chrome: Some(61 << 16),
        ..Browsers::default()
      }),
    );

    attr_test(
      "color: rgba(255, 0, 0, 0)",
      "color:rgba(255,0,0,0)",
      true,
      Some(Browsers {
        chrome: Some(61 << 16),
        ..Browsers::default()
      }),
    );

    attr_test(
      "color: rgba(255, 0, 0, 0)",
      "color:#f000",
      true,
      Some(Browsers {
        chrome: Some(62 << 16),
        ..Browsers::default()
      }),
    );

    prefix_test(
      ".foo { color: rgba(123, 456, 789, 0.5) }",
      indoc! { r#"
        .foo {
          color: #7bffff80;
        }
      "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { color: rgba(123, 255, 255, 0.5) }",
      indoc! { r#"
        .foo {
          color: rgba(123, 255, 255, .5);
        }
      "#},
      Browsers {
        ie: Some(11 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { color: #7bffff80 }",
      indoc! { r#"
        .foo {
          color: rgba(123, 255, 255, .5);
        }
      "#},
      Browsers {
        ie: Some(11 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { color: rgba(123, 456, 789, 0.5) }",
      indoc! { r#"
        .foo {
          color: rgba(123, 255, 255, .5);
        }
      "#},
      Browsers {
        firefox: Some(48 << 16),
        safari: Some(10 << 16),
        ios_saf: Some(9 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { color: rgba(123, 456, 789, 0.5) }",
      indoc! { r#"
        .foo {
          color: #7bffff80;
        }
      "#},
      Browsers {
        firefox: Some(49 << 16),
        safari: Some(10 << 16),
        ios_saf: Some(10 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          background-color: #b32323;
          background-color: lab(40% 56.6 39);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: lch(40% 68.735435 34.568626) }",
      indoc! { r#"
        .foo {
          background-color: #b32323;
          background-color: lch(40% 68.7354 34.5686);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: oklab(59.686% 0.1009 0.1192); }",
      indoc! { r#"
        .foo {
          background-color: #c65d07;
          background-color: lab(52.2319% 40.1449 59.9171);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: oklch(40% 0.1268735435 34.568626) }",
      indoc! { r#"
        .foo {
          background-color: #7e250f;
          background-color: lab(29.2661% 38.2437 35.3889);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          background-color: lab(40% 56.6 39);
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: oklab(59.686% 0.1009 0.1192); }",
      indoc! { r#"
        .foo {
          background-color: #c65d07;
          background-color: lab(52.2319% 40.1449 59.9171);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: oklab(59.686% 0.1009 0.1192); }",
      indoc! { r#"
        .foo {
          background-color: #c65d07;
          background-color: color(display-p3 .724144 .386777 .148795);
          background-color: lab(52.2319% 40.1449 59.9171);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: lab(40% 56.6 39) }",
      indoc! { r#"
        .foo {
          background-color: #b32323;
          background-color: color(display-p3 .643308 .192455 .167712);
          background-color: lab(40% 56.6 39);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: oklch(59.686% 0.15619 49.7694); }",
      indoc! { r#"
        .foo {
          background-color: #c65d06;
          background-color: lab(52.2321% 40.1417 59.9527);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(sRGB 0.41587 0.503670 0.36664); }",
      indoc! { r#"
        .foo {
          background-color: #6a805d;
          background-color: color(srgb .41587 .50367 .36664);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(display-p3 0.43313 0.50108 0.37950); }",
      indoc! { r#"
        .foo {
          background-color: #6a805d;
          background-color: color(display-p3 .43313 .50108 .3795);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(display-p3 0.43313 0.50108 0.37950); }",
      indoc! { r#"
        .foo {
          background-color: #6a805d;
          background-color: color(display-p3 .43313 .50108 .3795);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(display-p3 0.43313 0.50108 0.37950); }",
      indoc! { r#"
        .foo {
          background-color: color(display-p3 .43313 .50108 .3795);
        }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(display-p3 0.43313 0.50108 0.37950); }",
      indoc! { r#"
        .foo {
          background-color: #6a805d;
          background-color: color(display-p3 .43313 .50108 .3795);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(display-p3 0.43313 0.50108 0.37950); }",
      indoc! { r#"
        .foo {
          background-color: #6a805d;
          background-color: color(display-p3 .43313 .50108 .3795);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(a98-rgb 0.44091 0.49971 0.37408); }",
      indoc! { r#"
        .foo {
          background-color: #6a805d;
          background-color: color(a98-rgb .44091 .49971 .37408);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(a98-rgb 0.44091 0.49971 0.37408); }",
      indoc! { r#"
        .foo {
          background-color: color(a98-rgb .44091 .49971 .37408);
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(prophoto-rgb 0.36589 0.41717 0.31333); }",
      indoc! { r#"
        .foo {
          background-color: #6a805d;
          background-color: color(prophoto-rgb .36589 .41717 .31333);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(rec2020 0.42210 0.47580 0.35605); }",
      indoc! { r#"
        .foo {
          background-color: #728765;
          background-color: color(rec2020 .4221 .4758 .35605);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(xyz-d50 0.2005 0.14089 0.4472); }",
      indoc! { r#"
        .foo {
          background-color: #7654cd;
          background-color: color(xyz-d50 .2005 .14089 .4472);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: color(xyz-d65 0.21661 0.14602 0.59452); }",
      indoc! { r#"
        .foo {
          background-color: #7654cd;
          background-color: color(xyz .21661 .14602 .59452);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background-color: lch(50.998% 135.363 338) }",
      indoc! { r#"
        .foo {
          background-color: #ee00be;
          background-color: color(display-p3 .972962 -.362078 .804206);
          background-color: lch(50.998% 135.363 338);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { color: lch(50.998% 135.363 338) }",
      indoc! { r#"
        .foo {
          color: #ee00be;
          color: color(display-p3 .972962 -.362078 .804206);
          color: lch(50.998% 135.363 338);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { background: var(--image) lch(40% 68.735435 34.568626) }",
      indoc! { r#"
        .foo {
          background: var(--image) #b32323;
        }

        @supports (color: lab(0% 0 0)) {
          .foo {
            background: var(--image) lab(40% 56.6 39);
          }
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        color: red;
        color: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        color: red;
        color: lab(40% 56.6 39);
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        color: red;
        color: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        color: lab(40% 56.6 39);
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        color: var(--fallback);
        color: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        color: var(--fallback);
        color: lab(40% 56.6 39);
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        color: var(--fallback);
        color: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        color: lab(40% 56.6 39);
      }
    "#
      },
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        color: red;
        color: var(--foo, lab(40% 56.6 39));
      }
    "#,
      indoc! {r#"
      .foo {
        color: var(--foo, color(display-p3 .643308 .192455 .167712));
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          color: var(--foo, lab(40% 56.6 39));
        }
      }
    "#
      },
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --a: rgb(0 0 0 / var(--alpha));
        --b: rgb(50% 50% 50% / var(--alpha));
        --c: rgb(var(--x) 0 0);
        --d: rgb(0 var(--x) 0);
        --e: rgb(0 0 var(--x));
        --f: rgb(var(--x) 0 0 / var(--alpha));
        --g: rgb(0 var(--x) 0 / var(--alpha));
        --h: rgb(0 0 var(--x) / var(--alpha));
        --i: rgb(none 0 0 / var(--alpha));
        --j: rgb(from yellow r g b / var(--alpha));
      }
      "#,
      indoc! { r#"
        .foo {
          --a: rgba(0, 0, 0, var(--alpha));
          --b: rgba(128, 128, 128, var(--alpha));
          --c: rgb(var(--x) 0 0);
          --d: rgb(0 var(--x) 0);
          --e: rgb(0 0 var(--x));
          --f: rgb(var(--x) 0 0 / var(--alpha));
          --g: rgb(0 var(--x) 0 / var(--alpha));
          --h: rgb(0 0 var(--x) / var(--alpha));
          --i: rgb(none 0 0 / var(--alpha));
          --j: rgba(255, 255, 0, var(--alpha));
        }
      "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --a: rgb(0 0 0 / var(--alpha));
        --b: rgb(50% 50% 50% / var(--alpha));
        --c: rgb(var(--x) 0 0);
        --d: rgb(0 var(--x) 0);
        --e: rgb(0 0 var(--x));
        --f: rgb(var(--x) 0 0 / var(--alpha));
        --g: rgb(0 var(--x) 0 / var(--alpha));
        --h: rgb(0 0 var(--x) / var(--alpha));
        --i: rgb(none 0 0 / var(--alpha));
        --j: rgb(from yellow r g b / var(--alpha));
      }
      "#,
      indoc! { r#"
        .foo {
          --a: rgb(0 0 0 / var(--alpha));
          --b: rgb(128 128 128 / var(--alpha));
          --c: rgb(var(--x) 0 0);
          --d: rgb(0 var(--x) 0);
          --e: rgb(0 0 var(--x));
          --f: rgb(var(--x) 0 0 / var(--alpha));
          --g: rgb(0 var(--x) 0 / var(--alpha));
          --h: rgb(0 0 var(--x) / var(--alpha));
          --i: rgb(none 0 0 / var(--alpha));
          --j: rgb(255 255 0 / var(--alpha));
        }
      "#},
      Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --a: hsl(270 100% 50% / var(--alpha));
        --b: hsl(var(--x) 0 0);
        --c: hsl(0 var(--x) 0);
        --d: hsl(0 0 var(--x));
        --e: hsl(var(--x) 0 0 / var(--alpha));
        --f: hsl(0 var(--x) 0 / var(--alpha));
        --g: hsl(0 0 var(--x) / var(--alpha));
        --h: hsl(270 100% 50% / calc(var(--alpha) / 2));
        --i: hsl(none 100% 50% / var(--alpha));
        --j: hsl(from yellow h s l / var(--alpha));
      }
      "#,
      indoc! { r#"
        .foo {
          --a: hsla(270, 100%, 50%, var(--alpha));
          --b: hsl(var(--x) 0 0);
          --c: hsl(0 var(--x) 0);
          --d: hsl(0 0 var(--x));
          --e: hsl(var(--x) 0 0 / var(--alpha));
          --f: hsl(0 var(--x) 0 / var(--alpha));
          --g: hsl(0 0 var(--x) / var(--alpha));
          --h: hsla(270, 100%, 50%, calc(var(--alpha) / 2));
          --i: hsl(none 100% 50% / var(--alpha));
          --j: hsla(60, 100%, 50%, var(--alpha));
        }
      "#},
      Browsers {
        safari: Some(11 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --a: hsl(270 100% 50% / var(--alpha));
        --b: hsl(var(--x) 0 0);
        --c: hsl(0 var(--x) 0);
        --d: hsl(0 0 var(--x));
        --e: hsl(var(--x) 0 0 / var(--alpha));
        --f: hsl(0 var(--x) 0 / var(--alpha));
        --g: hsl(0 0 var(--x) / var(--alpha));
        --h: hsl(270 100% 50% / calc(var(--alpha) / 2));
        --i: hsl(none 100% 50% / var(--alpha));
      }
      "#,
      indoc! { r#"
        .foo {
          --a: hsl(270 100% 50% / var(--alpha));
          --b: hsl(var(--x) 0 0);
          --c: hsl(0 var(--x) 0);
          --d: hsl(0 0 var(--x));
          --e: hsl(var(--x) 0 0 / var(--alpha));
          --f: hsl(0 var(--x) 0 / var(--alpha));
          --g: hsl(0 0 var(--x) / var(--alpha));
          --h: hsl(270 100% 50% / calc(var(--alpha) / 2));
          --i: hsl(none 100% 50% / var(--alpha));
        }
      "#},
      Browsers {
        safari: Some(13 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
      .foo {
        --a: rgb(50% 50% 50% / calc(100% / 2));
        --b: hsl(calc(360deg / 2) 50% 50%);
        --c: oklab(40.101% calc(0.1 + 0.2) 0.0453);
        --d: color(display-p3 0.43313 0.50108 calc(0.1 + 0.2));
        --e: rgb(calc(255 / 2), calc(255 / 2), calc(255 / 2));
      }
      "#,
      indoc! { r#"
        .foo {
          --a: #80808080;
          --b: #40bfbf;
          --c: oklab(40.101% .3 .0453);
          --d: color(display-p3 .43313 .50108 .3);
          --e: gray;
        }
      "#},
    );
  }

  #[test]
  fn test_relative_color() {
    fn test(input: &str, output: &str) {
      let output = CssColor::parse_string(output)
        .unwrap()
        .to_css_string(PrinterOptions {
          minify: true,
          ..PrinterOptions::default()
        })
        .unwrap();
      minify_test(
        &format!(".foo {{ color: {} }}", input),
        &format!(".foo{{color:{}}}", output),
      );
    }

    test("lab(from indianred calc(l * .8) a b)", "lab(43.1402% 45.7516 23.1557)");
    test("lch(from indianred calc(l + 10%) c h)", "lch(63.9252% 51.2776 26.8448)");
    test("lch(from indianred l calc(c - 50) h)", "lch(53.9252% 1.27763 26.8448)");
    test(
      "lch(from indianred l c calc(h + 180deg))",
      "lch(53.9252% 51.2776 206.845)",
    );
    test("lch(from orchid l 30 h)", "lch(62.7526% 30 326.969)");
    test("lch(from orchid l 30 h)", "lch(62.7526% 30 326.969)");
    test("lch(from peru calc(l * 0.8) c h)", "lch(49.8022% 54.0117 63.6804)");
    test("rgb(from indianred 255 g b)", "rgb(255, 92, 92)");
    test("rgb(from indianred r g b / .5)", "rgba(205, 92, 92, .5)");
    test(
      "rgb(from rgba(205, 92, 92, .5) r g b / calc(alpha + .2))",
      "rgba(205, 92, 92, .7)",
    );
    test(
      "rgb(from rgba(205, 92, 92, .5) r g b / calc(alpha + 20%))",
      "rgba(205, 92, 92, .7)",
    );
    test("lch(from indianred l sin(c) h)", "lch(53.9252% .84797 26.8448)");
    test("lch(from indianred l sqrt(c) h)", "lch(53.9252% 7.16084 26.8448)");
    test("lch(from indianred l c sin(h))", "lch(53.9252% 51.2776 .990043)");
    minify_test(
      ".foo{color:lch(from currentColor l c sin(h))}",
      ".foo{color:lch(from currentColor l c sin(h))}",
    );

    // The following tests were converted from WPT: https://github.com/web-platform-tests/wpt/blob/master/css/css-color/parsing/relative-color-valid.html
    // Find: test_valid_value\(`color`, `(.*?)`,\s*`(.*?)`\)
    // Replace: test("$1", "$2")

    // Testing no modifications.
    test("rgb(from rebeccapurple r g b)", "#639");
    test("rgb(from rebeccapurple r g b / alpha)", "#639");
    test("rgb(from rgb(20%, 40%, 60%, 80%) r g b / alpha)", "#369c");
    test("rgb(from hsl(120deg 20% 50% / .5) r g b / alpha)", "#66996680");

    // Test nesting relative colors.
    test("rgb(from rgb(from rebeccapurple r g b) r g b)", "#639");

    // Testing non-sRGB origin colors to see gamut mapping.
    test("rgb(from color(display-p3 0 1 0) r g b / alpha)", "#00f942"); // Naive clip based mapping would give rgb(0, 255, 0).
    test("rgb(from lab(100% 104.3 -50.9) r g b)", "#fff"); // Naive clip based mapping would give rgb(255, 150, 255).
    test("rgb(from lab(0% 104.3 -50.9) r g b)", "#2a0022"); // Naive clip based mapping would give rgb(90, 0, 76). NOTE: 0% lightness in Lab/LCH does not automatically correspond with sRGB black.
    test("rgb(from lch(100% 116 334) r g b)", "#fff"); // Naive clip based mapping would give rgb(255, 150, 255).
    test("rgb(from lch(0% 116 334) r g b)", "#2a0022"); // Naive clip based mapping would give rgb(90, 0, 76). NOTE: 0% lightness in Lab/LCH does not automatically correspond with sRGB black.
    test("rgb(from oklab(100% 0.365 -0.16) r g b)", "#fff"); // Naive clip based mapping would give rgb(255, 92, 255).
    test("rgb(from oklab(0% 0.365 -0.16) r g b)", "#000"); // Naive clip based mapping would give rgb(19, 0, 24).
    test("rgb(from oklch(100% 0.399 336.3) r g b)", "#fff"); // Naive clip based mapping would give rgb(255, 91, 255).
    test("rgb(from oklch(0% 0.399 336.3) r g b)", "#000"); // Naive clip based mapping would give rgb(20, 0, 24).

    // Testing replacement with 0.
    test("rgb(from rebeccapurple 0 0 0)", "rgb(0, 0, 0)");
    test("rgb(from rebeccapurple 0 0 0 / 0)", "rgba(0, 0, 0, 0)");
    test("rgb(from rebeccapurple 0 g b / alpha)", "rgb(0, 51, 153)");
    test("rgb(from rebeccapurple r 0 b / alpha)", "rgb(102, 0, 153)");
    test("rgb(from rebeccapurple r g 0 / alpha)", "rgb(102, 51, 0)");
    test("rgb(from rebeccapurple r g b / 0)", "rgba(102, 51, 153, 0)");
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) 0 g b / alpha)",
      "rgba(0, 102, 153, 0.8)",
    );
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) r 0 b / alpha)",
      "rgba(51, 0, 153, 0.8)",
    );
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) r g 0 / alpha)",
      "rgba(51, 102, 0, 0.8)",
    );
    test("rgb(from rgb(20%, 40%, 60%, 80%) r g b / 0)", "rgba(51, 102, 153, 0)");

    // Testing replacement with a number.
    test("rgb(from rebeccapurple 25 g b / alpha)", "rgb(25, 51, 153)");
    test("rgb(from rebeccapurple r 25 b / alpha)", "rgb(102, 25, 153)");
    test("rgb(from rebeccapurple r g 25 / alpha)", "rgb(102, 51, 25)");
    test("rgb(from rebeccapurple r g b / .25)", "rgba(102, 51, 153, 0.25)");
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) 25 g b / alpha)",
      "rgba(25, 102, 153, 0.8)",
    );
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) r 25 b / alpha)",
      "rgba(51, 25, 153, 0.8)",
    );
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) r g 25 / alpha)",
      "rgba(51, 102, 25, 0.8)",
    );
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) r g b / .20)",
      "rgba(51, 102, 153, 0.2)",
    );

    // Testing replacement with a percentage.
    test("rgb(from rebeccapurple 20% g b / alpha)", "rgb(51, 51, 153)");
    test("rgb(from rebeccapurple r 20% b / alpha)", "rgb(102, 51, 153)");
    test("rgb(from rebeccapurple r g 20% / alpha)", "rgb(102, 51, 51)");
    test("rgb(from rebeccapurple r g b / 20%)", "rgba(102, 51, 153, 0.2)");
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) 20% g b / alpha)",
      "rgba(51, 102, 153, 0.8)",
    );
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) r 20% b / alpha)",
      "rgba(51, 51, 153, 0.8)",
    );
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) r g 20% / alpha)",
      "rgba(51, 102, 51, 0.8)",
    );
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) r g b / 20%)",
      "rgba(51, 102, 153, 0.2)",
    );

    // Testing replacement with a number for r, g, b but percent for alpha.
    test("rgb(from rebeccapurple 25 g b / 25%)", "rgba(25, 51, 153, 0.25)");
    test("rgb(from rebeccapurple r 25 b / 25%)", "rgba(102, 25, 153, 0.25)");
    test("rgb(from rebeccapurple r g 25 / 25%)", "rgba(102, 51, 25, 0.25)");
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) 25 g b / 25%)",
      "rgba(25, 102, 153, 0.25)",
    );
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) r 25 b / 25%)",
      "rgba(51, 25, 153, 0.25)",
    );
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) r g 25 / 25%)",
      "rgba(51, 102, 25, 0.25)",
    );

    // Testing permutation.
    test("rgb(from rebeccapurple g b r)", "rgb(51, 153, 102)");
    test("rgb(from rebeccapurple b alpha r / g)", "rgba(153, 255, 102, 0.2)");
    test("rgb(from rebeccapurple r r r / r)", "rgba(102, 102, 102, 0.4)");
    test(
      "rgb(from rebeccapurple alpha alpha alpha / alpha)",
      "rgb(255, 255, 255)",
    );
    test("rgb(from rgb(20%, 40%, 60%, 80%) g b r)", "rgb(102, 153, 51)");
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) b alpha r / g)",
      "rgba(153, 204, 51, 0.4)",
    );
    test("rgb(from rgb(20%, 40%, 60%, 80%) r r r / r)", "rgba(51, 51, 51, 0.2)");
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) alpha alpha alpha / alpha)",
      "rgba(204, 204, 204, 0.8)",
    );

    // Testing mixes of number and percentage. (These would not be allowed in the non-relative syntax).
    test("rgb(from rebeccapurple r 20% 10)", "rgb(102, 51, 10)");
    test("rgb(from rebeccapurple r 10 20%)", "rgb(102, 10, 51)");
    test("rgb(from rebeccapurple 0% 10 10)", "rgb(0, 10, 10)");
    test("rgb(from rgb(20%, 40%, 60%, 80%) r 20% 10)", "rgb(51, 51, 10)");
    test("rgb(from rgb(20%, 40%, 60%, 80%) r 10 20%)", "rgb(51, 10, 51)");
    test("rgb(from rgb(20%, 40%, 60%, 80%) 0% 10 10)", "rgb(0, 10, 10)");

    // Testing with calc().
    test("rgb(from rebeccapurple calc(r) calc(g) calc(b))", "rgb(102, 51, 153)");
    test("rgb(from rebeccapurple r calc(g * 2) 10)", "rgb(102, 102, 10)");
    test("rgb(from rebeccapurple b calc(r * .5) 10)", "rgb(153, 51, 10)");
    test("rgb(from rebeccapurple r calc(g * .5 + g * .5) 10)", "rgb(102, 51, 10)");
    test("rgb(from rebeccapurple r calc(b * .5 - g * .5) 10)", "rgb(102, 51, 10)");
    test(
      "rgb(from rgb(20%, 40%, 60%, 80%) calc(r) calc(g) calc(b) / calc(alpha))",
      "rgba(51, 102, 153, 0.8)",
    );

    // Testing with 'none'.
    test("rgb(from rebeccapurple none none none)", "rgb(0, 0, 0)");
    test("rgb(from rebeccapurple none none none / none)", "rgba(0, 0, 0, 0)");
    test("rgb(from rebeccapurple r g none)", "rgb(102, 51, 0)");
    test("rgb(from rebeccapurple r g none / alpha)", "rgb(102, 51, 0)");
    test("rgb(from rebeccapurple r g b / none)", "rgba(102, 51, 153, 0)");
    test(
      "rgb(from rgb(20% 40% 60% / 80%) r g none / alpha)",
      "rgba(51, 102, 0, 0.8)",
    );
    test("rgb(from rgb(20% 40% 60% / 80%) r g b / none)", "rgba(51, 102, 153, 0)");
    // FIXME: Clarify with spec editors if 'none' should pass through to the constants.
    test("rgb(from rgb(none none none) r g b)", "rgb(0, 0, 0)");
    test("rgb(from rgb(none none none / none) r g b / alpha)", "rgba(0, 0, 0, 0)");
    test("rgb(from rgb(20% none 60%) r g b)", "rgb(51, 0, 153)");
    test(
      "rgb(from rgb(20% 40% 60% / none) r g b / alpha)",
      "rgba(51, 102, 153, 0)",
    );

    // hsl(from ...)

    // Testing no modifications.
    test("hsl(from rebeccapurple h s l)", "rgb(102, 51, 153)");
    test("hsl(from rebeccapurple h s l / alpha)", "rgb(102, 51, 153)");
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) h s l / alpha)",
      "rgba(51, 102, 153, 0.8)",
    );
    test(
      "hsl(from hsl(120deg 20% 50% / .5) h s l / alpha)",
      "rgba(102, 153, 102, 0.5)",
    );

    // Test nesting relative colors.
    test("hsl(from hsl(from rebeccapurple h s l) h s l)", "rgb(102, 51, 153)");

    // Testing non-sRGB origin colors to see gamut mapping.
    test("hsl(from color(display-p3 0 1 0) h s l / alpha)", "rgb(0, 249, 66)"); // Naive clip based mapping would give rgb(0, 255, 0).
    test("hsl(from lab(100% 104.3 -50.9) h s l)", "rgb(255, 255, 255)"); // Naive clip based mapping would give rgb(255, 150, 255).
    test("hsl(from lab(0% 104.3 -50.9) h s l)", "rgb(42, 0, 34)"); // Naive clip based mapping would give rgb(90, 0, 76). NOTE: 0% lightness in Lab/LCH does not automatically correspond with sRGB black,
    test("hsl(from lch(100% 116 334) h s l)", "rgb(255, 255, 255)"); // Naive clip based mapping would give rgb(255, 150, 255).
    test("hsl(from lch(0% 116 334) h s l)", "rgb(42, 0, 34)"); // Naive clip based mapping would give rgb(90, 0, 76). NOTE: 0% lightness in Lab/LCH does not automatically correspond with sRGB black,
    test("hsl(from oklab(100% 0.365 -0.16) h s l)", "rgb(255, 255, 255)"); // Naive clip based mapping would give rgb(255, 92, 255).
    test("hsl(from oklab(0% 0.365 -0.16) h s l)", "rgb(0, 0, 0)"); // Naive clip based mapping would give rgb(19, 0, 24).
    test("hsl(from oklch(100% 0.399 336.3) h s l)", "rgb(255, 255, 255)"); // Naive clip based mapping would give rgb(255, 91, 255).
    test("hsl(from oklch(0% 0.399 336.3) h s l)", "rgb(0, 0, 0)"); // Naive clip based mapping would give rgb(20, 0, 24).

    // Testing replacement with 0.
    test("hsl(from rebeccapurple 0 0% 0%)", "rgb(0, 0, 0)");
    test("hsl(from rebeccapurple 0deg 0% 0%)", "rgb(0, 0, 0)");
    test("hsl(from rebeccapurple 0 0% 0% / 0)", "rgba(0, 0, 0, 0)");
    test("hsl(from rebeccapurple 0deg 0% 0% / 0)", "rgba(0, 0, 0, 0)");
    test("hsl(from rebeccapurple 0 s l / alpha)", "rgb(153, 51, 51)");
    test("hsl(from rebeccapurple 0deg s l / alpha)", "rgb(153, 51, 51)");
    test("hsl(from rebeccapurple h 0% l / alpha)", "rgb(102, 102, 102)");
    test("hsl(from rebeccapurple h s 0% / alpha)", "rgb(0, 0, 0)");
    test("hsl(from rebeccapurple h s l / 0)", "rgba(102, 51, 153, 0)");
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) 0 s l / alpha)",
      "rgba(153, 51, 51, 0.8)",
    );
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) 0deg s l / alpha)",
      "rgba(153, 51, 51, 0.8)",
    );
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) h 0% l / alpha)",
      "rgba(102, 102, 102, 0.8)",
    );
    test("hsl(from rgb(20%, 40%, 60%, 80%) h s 0% / alpha)", "rgba(0, 0, 0, 0.8)");
    test("hsl(from rgb(20%, 40%, 60%, 80%) h s l / 0)", "rgba(51, 102, 153, 0)");

    // Testing replacement with a constant.
    test("hsl(from rebeccapurple 25 s l / alpha)", "rgb(153, 94, 51)");
    test("hsl(from rebeccapurple 25deg s l / alpha)", "rgb(153, 94, 51)");
    test("hsl(from rebeccapurple h 20% l / alpha)", "rgb(102, 82, 122)");
    test("hsl(from rebeccapurple h s 20% / alpha)", "rgb(51, 25, 77)");
    test("hsl(from rebeccapurple h s l / .25)", "rgba(102, 51, 153, 0.25)");
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) 25 s l / alpha)",
      "rgba(153, 94, 51, 0.8)",
    );
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) 25deg s l / alpha)",
      "rgba(153, 94, 51, 0.8)",
    );
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) h 20% l / alpha)",
      "rgba(82, 102, 122, 0.8)",
    );
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) h s 20% / alpha)",
      "rgba(25, 51, 77, 0.8)",
    );
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) h s l / .2)",
      "rgba(51, 102, 153, 0.2)",
    );

    // Testing valid permutation (types match).
    test("hsl(from rebeccapurple h l s)", "rgb(128, 77, 179)");
    test("hsl(from rebeccapurple h alpha l / s)", "rgba(102, 0, 204, 0.5)");
    test("hsl(from rebeccapurple h l l / l)", "rgba(102, 61, 143, 0.4)");
    test("hsl(from rebeccapurple h alpha alpha / alpha)", "rgb(255, 255, 255)");
    test("hsl(from rgb(20%, 40%, 60%, 80%) h l s)", "rgb(77, 128, 179)");
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) h alpha l / s)",
      "rgba(20, 102, 184, 0.5)",
    );
    test("hsl(from rgb(20%, 40%, 60%, 80%) h l l / l)", "rgba(61, 102, 143, 0.4)");
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) h alpha alpha / alpha)",
      "rgba(163, 204, 245, 0.8)",
    );

    // Testing with calc().
    test("hsl(from rebeccapurple calc(h) calc(s) calc(l))", "rgb(102, 51, 153)");
    test(
      "hsl(from rgb(20%, 40%, 60%, 80%) calc(h) calc(s) calc(l) / calc(alpha))",
      "rgba(51, 102, 153, 0.8)",
    );

    // Testing with 'none'.
    test("hsl(from rebeccapurple none none none)", "rgb(0, 0, 0)");
    test("hsl(from rebeccapurple none none none / none)", "rgba(0, 0, 0, 0)");
    test("hsl(from rebeccapurple h s none)", "rgb(0, 0, 0)");
    test("hsl(from rebeccapurple h s none / alpha)", "rgb(0, 0, 0)");
    test("hsl(from rebeccapurple h s l / none)", "rgba(102, 51, 153, 0)");
    test("hsl(from rebeccapurple none s l / alpha)", "rgb(153, 51, 51)");
    test(
      "hsl(from hsl(120deg 20% 50% / .5) h s none / alpha)",
      "rgba(0, 0, 0, 0.5)",
    );
    test(
      "hsl(from hsl(120deg 20% 50% / .5) h s l / none)",
      "rgba(102, 153, 102, 0)",
    );
    test(
      "hsl(from hsl(120deg 20% 50% / .5) none s l / alpha)",
      "rgba(153, 102, 102, 0.5)",
    );
    // FIXME: Clarify with spec editors if 'none' should pass through to the constants.
    test("hsl(from hsl(none none none) h s l)", "rgb(0, 0, 0)");
    test("hsl(from hsl(none none none / none) h s l / alpha)", "rgba(0, 0, 0, 0)");
    test("hsl(from hsl(120deg none 50% / .5) h s l)", "rgb(128, 128, 128)");
    test(
      "hsl(from hsl(120deg 20% 50% / none) h s l / alpha)",
      "rgba(102, 153, 102, 0)",
    );
    test(
      "hsl(from hsl(none 20% 50% / .5) h s l / alpha)",
      "rgba(153, 102, 102, 0.5)",
    );

    // hwb(from ...)

    // Testing no modifications.
    test("hwb(from rebeccapurple h w b)", "rgb(102, 51, 153)");
    test("hwb(from rebeccapurple h w b / alpha)", "rgb(102, 51, 153)");
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) h w b / alpha)",
      "rgba(51, 102, 153, 0.8)",
    );
    test(
      "hwb(from hsl(120deg 20% 50% / .5) h w b / alpha)",
      "rgba(102, 153, 102, 0.5)",
    );

    // Test nesting relative colors.
    test("hwb(from hwb(from rebeccapurple h w b) h w b)", "rgb(102, 51, 153)");

    // Testing non-sRGB origin colors to see gamut mapping.
    test("hwb(from color(display-p3 0 1 0) h w b / alpha)", "rgb(0, 249, 66)"); // Naive clip based mapping would give rgb(0, 255, 0).
    test("hwb(from lab(100% 104.3 -50.9) h w b)", "rgb(255, 255, 255)"); // Naive clip based mapping would give rgb(255, 150, 255).
    test("hwb(from lab(0% 104.3 -50.9) h w b)", "rgb(42, 0, 34)"); // Naive clip based mapping would give rgb(90, 0, 76). NOTE: 0% lightness in Lab/LCH does not automatically correspond with sRGB black,
    test("hwb(from lch(100% 116 334) h w b)", "rgb(255, 255, 255)"); // Naive clip based mapping would give rgb(255, 150, 255).
    test("hwb(from lch(0% 116 334) h w b)", "rgb(42, 0, 34)"); // Naive clip based mapping would give rgb(90, 0, 76). NOTE: 0% lightness in Lab/LCH does not automatically correspond with sRGB black,
    test("hwb(from oklab(100% 0.365 -0.16) h w b)", "rgb(255, 255, 255)"); // Naive clip based mapping would give rgb(255, 92, 255).
    test("hwb(from oklab(0% 0.365 -0.16) h w b)", "rgb(0, 0, 0)"); // Naive clip based mapping would give rgb(19, 0, 24).
    test("hwb(from oklch(100% 0.399 336.3) h w b)", "rgb(255, 255, 255)"); // Naive clip based mapping would give rgb(255, 91, 255).
    test("hwb(from oklch(0% 0.399 336.3) h w b)", "rgb(0, 0, 0)"); // Naive clip based mapping would give rgb(20, 0, 24).

    // Testing replacement with 0.
    test("hwb(from rebeccapurple 0 0% 0%)", "rgb(255, 0, 0)");
    test("hwb(from rebeccapurple 0deg 0% 0%)", "rgb(255, 0, 0)");
    test("hwb(from rebeccapurple 0 0% 0% / 0)", "rgba(255, 0, 0, 0)");
    test("hwb(from rebeccapurple 0deg 0% 0% / 0)", "rgba(255, 0, 0, 0)");
    test("hwb(from rebeccapurple 0 w b / alpha)", "rgb(153, 51, 51)");
    test("hwb(from rebeccapurple 0deg w b / alpha)", "rgb(153, 51, 51)");
    test("hwb(from rebeccapurple h 0% b / alpha)", "rgb(77, 0, 153)");
    test("hwb(from rebeccapurple h w 0% / alpha)", "rgb(153, 51, 255)");
    test("hwb(from rebeccapurple h w b / 0)", "rgba(102, 51, 153, 0)");
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) 0 w b / alpha)",
      "rgba(153, 51, 51, 0.8)",
    );
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) 0deg w b / alpha)",
      "rgba(153, 51, 51, 0.8)",
    );
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) h 0% b / alpha)",
      "rgba(0, 77, 153, 0.8)",
    );
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) h w 0% / alpha)",
      "rgba(51, 153, 255, 0.8)",
    );
    test("hwb(from rgb(20%, 40%, 60%, 80%) h w b / 0)", "rgba(51, 102, 153, 0)");

    // Testing replacement with a constant.
    test("hwb(from rebeccapurple 25 w b / alpha)", "rgb(153, 94, 51)");
    test("hwb(from rebeccapurple 25deg w b / alpha)", "rgb(153, 94, 51)");
    test("hwb(from rebeccapurple h 20% b / alpha)", "rgb(102, 51, 153)");
    test("hwb(from rebeccapurple h w 20% / alpha)", "rgb(128, 51, 204)");
    test("hwb(from rebeccapurple h w b / .2)", "rgba(102, 51, 153, 0.2)");
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) 25 w b / alpha)",
      "rgba(153, 94, 51, 0.8)",
    );
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) 25deg w b / alpha)",
      "rgba(153, 94, 51, 0.8)",
    );
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) h 20% b / alpha)",
      "rgba(51, 102, 153, 0.8)",
    );
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) h w 20% / alpha)",
      "rgba(51, 128, 204, 0.8)",
    );
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) h w b / .2)",
      "rgba(51, 102, 153, 0.2)",
    );

    // Testing valid permutation (types match).
    test("hwb(from rebeccapurple h b w)", "rgb(153, 102, 204)");
    test("hwb(from rebeccapurple h alpha w / b)", "rgba(213, 213, 213, 0.4)");
    test("hwb(from rebeccapurple h w w / w)", "rgba(128, 51, 204, 0.2)");
    test("hwb(from rebeccapurple h alpha alpha / alpha)", "rgb(128, 128, 128)");
    test("hwb(from rgb(20%, 40%, 60%, 80%) h b w)", "rgb(102, 153, 204)");
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) h alpha w / b)",
      "rgba(204, 204, 204, 0.4)",
    );
    test("hwb(from rgb(20%, 40%, 60%, 80%) h w w / w)", "rgba(51, 128, 204, 0.2)");
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) h alpha alpha / alpha)",
      "rgba(128, 128, 128, 0.8)",
    );

    // Testing with calc().
    test("hwb(from rebeccapurple calc(h) calc(w) calc(b))", "rgb(102, 51, 153)");
    test(
      "hwb(from rgb(20%, 40%, 60%, 80%) calc(h) calc(w) calc(b) / calc(alpha))",
      "rgba(51, 102, 153, 0.8)",
    );

    // Testing with 'none'.
    test("hwb(from rebeccapurple none none none)", "rgb(255, 0, 0)");
    test("hwb(from rebeccapurple none none none / none)", "rgba(255, 0, 0, 0)");
    test("hwb(from rebeccapurple h w none)", "rgb(153, 51, 255)");
    test("hwb(from rebeccapurple h w none / alpha)", "rgb(153, 51, 255)");
    test("hwb(from rebeccapurple h w b / none)", "rgba(102, 51, 153, 0)");
    test("hwb(from rebeccapurple none w b / alpha)", "rgb(153, 51, 51)");
    test(
      "hwb(from hwb(120deg 20% 50% / .5) h w none / alpha)",
      "rgba(51, 255, 51, 0.5)",
    );
    test(
      "hwb(from hwb(120deg 20% 50% / .5) h w b / none)",
      "rgba(51, 128, 51, 0)",
    );
    test(
      "hwb(from hwb(120deg 20% 50% / .5) none w b / alpha)",
      "rgba(128, 51, 51, 0.5)",
    );
    // FIXME: Clarify with spec editors if 'none' should pass through to the constants.
    test("hwb(from hwb(none none none) h w b)", "rgb(255, 0, 0)");
    test(
      "hwb(from hwb(none none none / none) h w b / alpha)",
      "rgba(255, 0, 0, 0)",
    );
    test("hwb(from hwb(120deg none 50% / .5) h w b)", "rgb(0, 128, 0)");
    test(
      "hwb(from hwb(120deg 20% 50% / none) h w b / alpha)",
      "rgba(51, 128, 51, 0)",
    );
    test(
      "hwb(from hwb(none 20% 50% / .5) h w b / alpha)",
      "rgba(128, 51, 51, 0.5)",
    );

    for color_space in &["lab", "oklab"] {
      // Testing no modifications.
      test(
        &format!("{}(from {}(25% 20 50) l a b)", color_space, color_space),
        &format!("{}(25% 20 50)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l a b / alpha)", color_space, color_space),
        &format!("{}(25% 20 50)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) l a b / alpha)", color_space, color_space),
        &format!("{}(25% 20 50 / 0.4)", color_space),
      );
      test(
        &format!(
          "{}(from {}(200% 300 400 / 500%) l a b / alpha)",
          color_space, color_space
        ),
        &format!("{}(200% 300 400)", color_space),
      );
      test(
        &format!(
          "{}(from {}(-200% -300 -400 / -500%) l a b / alpha)",
          color_space, color_space
        ),
        &format!("{}(0% -300 -400 / 0)", color_space),
      );

      // Test nesting relative colors.
      test(
        &format!(
          "{}(from {}(from {}(25% 20 50) l a b) l a b)",
          color_space, color_space, color_space
        ),
        &format!("{}(25% 20 50)", color_space),
      );

      // Testing non-${colorSpace} origin to see conversion.
      test(
        &format!("{}(from color(display-p3 0 0 0) l a b / alpha)", color_space),
        &format!("{}(0% 0 0)", color_space),
      );

      // Testing replacement with 0.
      test(
        &format!("{}(from {}(25% 20 50) 0% 0 0)", color_space, color_space),
        &format!("{}(0% 0 0)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) 0% 0 0 / 0)", color_space, color_space),
        &format!("{}(0% 0 0 / 0)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) 0% a b / alpha)", color_space, color_space),
        &format!("{}(0% 20 50)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l 0 b / alpha)", color_space, color_space),
        &format!("{}(25% 0 50)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l a 0 / alpha)", color_space, color_space),
        &format!("{}(25% 20 0)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l a b / 0)", color_space, color_space),
        &format!("{}(25% 20 50 / 0)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) 0% a b / alpha)", color_space, color_space),
        &format!("{}(0% 20 50 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) l 0 b / alpha)", color_space, color_space),
        &format!("{}(25% 0 50 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) l a 0 / alpha)", color_space, color_space),
        &format!("{}(25% 20 0 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) l a b / 0)", color_space, color_space),
        &format!("{}(25% 20 50 / 0)", color_space),
      );

      // Testing replacement with a constant.
      test(
        &format!("{}(from {}(25% 20 50) 35% a b / alpha)", color_space, color_space),
        &format!("{}(35% 20 50)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l 35 b / alpha)", color_space, color_space),
        &format!("{}(25% 35 50)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l a 35 / alpha)", color_space, color_space),
        &format!("{}(25% 20 35)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l a b / .35)", color_space, color_space),
        &format!("{}(25% 20 50 / 0.35)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) 35% a b / alpha)", color_space, color_space),
        &format!("{}(35% 20 50 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) l 35 b / alpha)", color_space, color_space),
        &format!("{}(25% 35 50 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) l a 35 / alpha)", color_space, color_space),
        &format!("{}(25% 20 35 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) l a b / .35)", color_space, color_space),
        &format!("{}(25% 20 50 / 0.35)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) 200% 300 400 / 500)",
          color_space, color_space
        ),
        &format!("{}(200% 300 400)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) -200% -300 -400 / -500)",
          color_space, color_space
        ),
        &format!("{}(0% -300 -400 / 0)", color_space),
      );

      // Testing valid permutation (types match).
      test(
        &format!("{}(from {}(25% 20 50) l b a)", color_space, color_space),
        &format!("{}(25% 50 20)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l a a / a)", color_space, color_space),
        &format!("{}(25% 20 20)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) l b a)", color_space, color_space),
        &format!("{}(25% 50 20)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) l a a / a)", color_space, color_space),
        &format!("{}(25% 20 20)", color_space),
      );

      // Testing with calc().
      test(
        &format!(
          "{}(from {}(25% 20 50) calc(l) calc(a) calc(b))",
          color_space, color_space
        ),
        &format!("{}(25% 20 50)", color_space),
      );
      test(
        &format!(
          "{}(from {}(25% 20 50 / 40%) calc(l) calc(a) calc(b) / calc(alpha))",
          color_space, color_space
        ),
        &format!("{}(25% 20 50 / 0.4)", color_space),
      );

      // Testing with 'none'.
      test(
        &format!("{}(from {}(25% 20 50) none none none)", color_space, color_space),
        &format!("{}(none none none)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) none none none / none)", color_space, color_space),
        &format!("{}(none none none / none)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l a none)", color_space, color_space),
        &format!("{}(25% 20 none)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l a none / alpha)", color_space, color_space),
        &format!("{}(25% 20 none)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50) l a b / none)", color_space, color_space),
        &format!("{}(25% 20 50 / none)", color_space),
      );
      test(
        &format!(
          "{}(from {}(25% 20 50 / 40%) l a none / alpha)",
          color_space, color_space
        ),
        &format!("{}(25% 20 none / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / 40%) l a b / none)", color_space, color_space),
        &format!("{}(25% 20 50 / none)", color_space),
      );
      // FIXME: Clarify with spec editors if 'none' should pass through to the constants.
      test(
        &format!("{}(from {}(none none none) l a b)", color_space, color_space),
        &format!("{}(0% 0 0)", color_space),
      );
      test(
        &format!(
          "{}(from {}(none none none / none) l a b / alpha)",
          color_space, color_space
        ),
        &format!("{}(0% 0 0 / 0)", color_space),
      );
      test(
        &format!("{}(from {}(25% none 50) l a b)", color_space, color_space),
        &format!("{}(25% 0 50)", color_space),
      );
      test(
        &format!("{}(from {}(25% 20 50 / none) l a b / alpha)", color_space, color_space),
        &format!("{}(25% 20 50 / 0)", color_space),
      );
    }

    // test_valid_value\(`color`, `\$\{colorSpace\}\(from \$\{colorSpace\}\((.*?)`,\s*`\$\{colorSpace\}(.*?)`\)
    // test(&format!("{}(from {}($1", color_space, color_space), &format!("{}$2", color_space))

    for color_space in &["lch", "oklch"] {
      // Testing no modifications.
      test(
        &format!("{}(from {}(70% 45 30) l c h)", color_space, color_space),
        &format!("{}(70% 45 30)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c h / alpha)", color_space, color_space),
        &format!("{}(70% 45 30)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) l c h / alpha)", color_space, color_space),
        &format!("{}(70% 45 30 / 0.4)", color_space),
      );
      test(
        &format!(
          "{}(from {}(200% 300 400 / 500%) l c h / alpha)",
          color_space, color_space
        ),
        &format!("{}(200% 300 40)", color_space),
      );
      test(
        &format!(
          "{}(from {}(-200% -300 -400 / -500%) l c h / alpha)",
          color_space, color_space
        ),
        &format!("{}(0% 0 320 / 0)", color_space),
      );

      // Test nesting relative colors.
      test(
        &format!(
          "{}(from {}(from {}(70% 45 30) l c h) l c h)",
          color_space, color_space, color_space
        ),
        &format!("{}(70% 45 30)", color_space),
      );

      // Testing non-sRGB origin colors (no gamut mapping will happen since the destination is not a bounded RGB color space).
      test(
        &format!("{}(from color(display-p3 0 0 0) l c h / alpha)", color_space),
        &format!("{}(0% 0 0)", color_space),
      );

      // Testing replacement with 0.
      test(
        &format!("{}(from {}(70% 45 30) 0% 0 0)", color_space, color_space),
        &format!("{}(0% 0 0)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) 0% 0 0deg)", color_space, color_space),
        &format!("{}(0% 0 0)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) 0% 0 0 / 0)", color_space, color_space),
        &format!("{}(0% 0 0 / 0)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) 0% 0 0deg / 0)", color_space, color_space),
        &format!("{}(0% 0 0 / 0)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) 0% c h / alpha)", color_space, color_space),
        &format!("{}(0% 45 30)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l 0 h / alpha)", color_space, color_space),
        &format!("{}(70% 0 30)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c 0 / alpha)", color_space, color_space),
        &format!("{}(70% 45 0)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c 0deg / alpha)", color_space, color_space),
        &format!("{}(70% 45 0)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c h / 0)", color_space, color_space),
        &format!("{}(70% 45 30 / 0)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) 0% c h / alpha)", color_space, color_space),
        &format!("{}(0% 45 30 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) l 0 h / alpha)", color_space, color_space),
        &format!("{}(70% 0 30 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) l c 0 / alpha)", color_space, color_space),
        &format!("{}(70% 45 0 / 0.4)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) l c 0deg / alpha)",
          color_space, color_space
        ),
        &format!("{}(70% 45 0 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) l c h / 0)", color_space, color_space),
        &format!("{}(70% 45 30 / 0)", color_space),
      );

      // Testing replacement with a constant.
      test(
        &format!("{}(from {}(70% 45 30) 25% c h / alpha)", color_space, color_space),
        &format!("{}(25% 45 30)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l 25 h / alpha)", color_space, color_space),
        &format!("{}(70% 25 30)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c 25 / alpha)", color_space, color_space),
        &format!("{}(70% 45 25)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c 25deg / alpha)", color_space, color_space),
        &format!("{}(70% 45 25)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c h / .25)", color_space, color_space),
        &format!("{}(70% 45 30 / 0.25)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) 25% c h / alpha)", color_space, color_space),
        &format!("{}(25% 45 30 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) l 25 h / alpha)", color_space, color_space),
        &format!("{}(70% 25 30 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) l c 25 / alpha)", color_space, color_space),
        &format!("{}(70% 45 25 / 0.4)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) l c 25deg / alpha)",
          color_space, color_space
        ),
        &format!("{}(70% 45 25 / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) l c h / .25)", color_space, color_space),
        &format!("{}(70% 45 30 / 0.25)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) 200% 300 400 / 500)",
          color_space, color_space
        ),
        &format!("{}(200% 300 400)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) -200% -300 -400 / -500)",
          color_space, color_space
        ),
        &format!("{}(0% 0 -400 / 0)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) 50% 120 400deg / 500)",
          color_space, color_space
        ),
        &format!("{}(50% 120 400)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) 50% 120 -400deg / -500)",
          color_space, color_space
        ),
        &format!("{}(50% 120 -400 / 0)", color_space),
      );

      // Testing valid permutation (types match).
      // NOTE: 'c' is a valid hue, as hue is <angle>|<number>.
      test(
        &format!("{}(from {}(70% 45 30) alpha c h / l)", color_space, color_space),
        &format!("{}(100% 45 30 / 0.7)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c c / alpha)", color_space, color_space),
        &format!("{}(70% 45 45)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) alpha c h / alpha)", color_space, color_space),
        &format!("{}(100% 45 30)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) alpha c c / alpha)", color_space, color_space),
        &format!("{}(100% 45 45)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) alpha c h / l)", color_space, color_space),
        &format!("{}(40% 45 30 / 0.7)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) l c c / alpha)", color_space, color_space),
        &format!("{}(70% 45 45 / 0.4)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) alpha c h / alpha)",
          color_space, color_space
        ),
        &format!("{}(40% 45 30 / 0.4)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) alpha c c / alpha)",
          color_space, color_space
        ),
        &format!("{}(40% 45 45 / 0.4)", color_space),
      );

      // Testing with calc().
      test(
        &format!(
          "{}(from {}(70% 45 30) calc(l) calc(c) calc(h))",
          color_space, color_space
        ),
        &format!("{}(70% 45 30)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) calc(l) calc(c) calc(h) / calc(alpha))",
          color_space, color_space
        ),
        &format!("{}(70% 45 30 / 0.4)", color_space),
      );

      // Testing with 'none'.
      test(
        &format!("{}(from {}(70% 45 30) none none none)", color_space, color_space),
        &format!("{}(none none none)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) none none none / none)", color_space, color_space),
        &format!("{}(none none none / none)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c none)", color_space, color_space),
        &format!("{}(70% 45 none)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c none / alpha)", color_space, color_space),
        &format!("{}(70% 45 none)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30) l c h / none)", color_space, color_space),
        &format!("{}(70% 45 30 / none)", color_space),
      );
      test(
        &format!(
          "{}(from {}(70% 45 30 / 40%) l c none / alpha)",
          color_space, color_space
        ),
        &format!("{}(70% 45 none / 0.4)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / 40%) l c h / none)", color_space, color_space),
        &format!("{}(70% 45 30 / none)", color_space),
      );
      // FIXME: Clarify with spec editors if 'none' should pass through to the constants.
      test(
        &format!("{}(from {}(none none none) l c h)", color_space, color_space),
        &format!("{}(0% 0 0)", color_space),
      );
      test(
        &format!(
          "{}(from {}(none none none / none) l c h / alpha)",
          color_space, color_space
        ),
        &format!("{}(0% 0 0 / 0)", color_space),
      );
      test(
        &format!("{}(from {}(70% none 30) l c h)", color_space, color_space),
        &format!("{}(70% 0 30)", color_space),
      );
      test(
        &format!("{}(from {}(70% 45 30 / none) l c h / alpha)", color_space, color_space),
        &format!("{}(70% 45 30 / 0)", color_space),
      );
    }

    // test_valid_value\(`color`, `color\(from color\(\$\{colorSpace\}(.*?) \$\{colorSpace\}(.*?)`,\s*`color\(\$\{colorSpace\}(.*?)`\)
    // test(&format!("color(from color({}$1 {}$2", color_space, color_space), &format!("color({}$3", color_space))

    for color_space in &["srgb", "srgb-linear", "a98-rgb", "rec2020", "prophoto-rgb"] {
      // Testing no modifications.
      test(
        &format!("color(from color({} 0.7 0.5 0.3) {} r g b)", color_space, color_space),
        &format!("color({} 0.7 0.5 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r g b)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / 0.4)", color_space),
      );

      // Test nesting relative colors.
      test(
        &format!(
          "color(from color(from color({} 0.7 0.5 0.3) {} r g b) {} r g b)",
          color_space, color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3)", color_space),
      );

      // Testing replacement with 0.
      test(
        &format!("color(from color({} 0.7 0.5 0.3) {} 0 0 0)", color_space, color_space),
        &format!("color({} 0 0 0)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} 0 0 0 / 0)",
          color_space, color_space
        ),
        &format!("color({} 0 0 0 / 0)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} 0 g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0 0.5 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r 0 b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r g 0 / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r g b / 0)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / 0)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} 0 g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0 0.5 0.3 / 0.4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r 0 b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0 0.3 / 0.4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r g 0 / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0 / 0.4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r g b / 0)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / 0)", color_space),
      );

      // Testing replacement with a constant.
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} 0.2 g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.2 0.5 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} 20% g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.2 0.5 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r 0.2 b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.2 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r 20% b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.2 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r g 0.2 / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.2)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r g 20% / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.2)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r g b / 0.2)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / 0.2)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r g b / 20%)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / 0.2)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} 0.2 g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.2 0.5 0.3 / 0.4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} 20% g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.2 0.5 0.3 / 0.4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r 0.2 b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.2 0.3 / 0.4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r 20% b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.2 0.3 / 0.4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r g 0.2 / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.2 / 0.4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r g 20% / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.2 / 0.4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r g b / 0.2)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / 0.2)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r g b / 20%)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / 0.2)", color_space),
      );
      test(
        &format!("color(from color({} 0.7 0.5 0.3) {} 2 3 4)", color_space, color_space),
        &format!("color({} 2 3 4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} 2 3 4 / 5)",
          color_space, color_space
        ),
        &format!("color({} 2 3 4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} -2 -3 -4)",
          color_space, color_space
        ),
        &format!("color({} -2 -3 -4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} -2 -3 -4 / -5)",
          color_space, color_space
        ),
        &format!("color({} -2 -3 -4 / 0)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} 200% 300% 400%)",
          color_space, color_space
        ),
        &format!("color({} 2 3 4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} 200% 300% 400% / 500%)",
          color_space, color_space
        ),
        &format!("color({} 2 3 4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} -200% -300% -400%)",
          color_space, color_space
        ),
        &format!("color({} -2 -3 -4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} -200% -300% -400% / -500%)",
          color_space, color_space
        ),
        &format!("color({} -2 -3 -4 / 0)", color_space),
      );

      // Testing valid permutation (types match).
      test(
        &format!("color(from color({} 0.7 0.5 0.3) {} g b r)", color_space, color_space),
        &format!("color({} 0.5 0.3 0.7)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} b alpha r / g)",
          color_space, color_space
        ),
        &format!("color({} 0.3 1 0.7 / 0.5)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r r r / r)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.7 0.7 / 0.7)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} alpha alpha alpha / alpha)",
          color_space, color_space
        ),
        &format!("color({} 1 1 1)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} g b r)",
          color_space, color_space
        ),
        &format!("color({} 0.5 0.3 0.7)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} b alpha r / g)",
          color_space, color_space
        ),
        &format!("color({} 0.3 0.4 0.7 / 0.5)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r r r / r)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.7 0.7 / 0.7)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} alpha alpha alpha / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.4 0.4 0.4 / 0.4)", color_space),
      );

      // Testing out of gamut components.
      test(
        &format!("color(from color({} 1.7 1.5 1.3) {} r g b)", color_space, color_space),
        &format!("color({} 1.7 1.5 1.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 1.7 1.5 1.3) {} r g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 1.7 1.5 1.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 1.7 1.5 1.3 / 140%) {} r g b)",
          color_space, color_space
        ),
        &format!("color({} 1.7 1.5 1.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 1.7 1.5 1.3 / 140%) {} r g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 1.7 1.5 1.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} -0.7 -0.5 -0.3) {} r g b)",
          color_space, color_space
        ),
        &format!("color({} -0.7 -0.5 -0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} -0.7 -0.5 -0.3) {} r g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} -0.7 -0.5 -0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} -0.7 -0.5 -0.3 / -40%) {} r g b)",
          color_space, color_space
        ),
        &format!("color({} -0.7 -0.5 -0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} -0.7 -0.5 -0.3 / -40%) {} r g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} -0.7 -0.5 -0.3 / 0)", color_space),
      );

      // Testing with calc().
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} calc(r) calc(g) calc(b))",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} calc(r) calc(g) calc(b) / calc(alpha))",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / 0.4)", color_space),
      );

      // Testing with 'none'.
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} none none none)",
          color_space, color_space
        ),
        &format!("color({} none none none)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} none none none / none)",
          color_space, color_space
        ),
        &format!("color({} none none none / none)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r g none)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 none)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r g none / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 none)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3) {} r g b / none)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / none)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r g none / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 none / 0.4)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / 40%) {} r g b / none)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / none)", color_space),
      );
      // FIXME: Clarify with spec editors if 'none' should pass through to the constants.
      test(
        &format!(
          "color(from color({} none none none) {} r g b)",
          color_space, color_space
        ),
        &format!("color({} 0 0 0)", color_space),
      );
      test(
        &format!(
          "color(from color({} none none none / none) {} r g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0 0 0 / 0)", color_space),
      );
      test(
        &format!("color(from color({} 0.7 none 0.3) {} r g b)", color_space, color_space),
        &format!("color({} 0.7 0 0.3)", color_space),
      );
      test(
        &format!(
          "color(from color({} 0.7 0.5 0.3 / none) {} r g b / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.7 0.5 0.3 / 0)", color_space),
      );
    }

    // test_valid_value\(`color`, `color\(from color\(\$\{colorSpace\}(.*?) \$\{colorSpace\}(.*?)`,\s*`color\(\$\{resultColorSpace\}(.*?)`\)
    // test(&format!("color(from color({}$1 {}$2", color_space, color_space), &format!("color({}$3", result_color_space))

    for color_space in &["xyz", "xyz-d50", "xyz-d65"] {
      let result_color_space = if *color_space == "xyz" { "xyz-d65" } else { color_space };

      // Testing no modifications.
      test(
        &format!("color(from color({} 7 -20.5 100) {} x y z)", color_space, color_space),
        &format!("color({} 7 -20.5 100)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x y z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x y z)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x y z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100 / 0.4)", result_color_space),
      );

      // Test nesting relative colors.
      test(
        &format!(
          "color(from color(from color({} 7 -20.5 100) {} x y z) {} x y z)",
          color_space, color_space, color_space
        ),
        &format!("color({} 7 -20.5 100)", result_color_space),
      );

      // Testing replacement with 0.
      test(
        &format!("color(from color({} 7 -20.5 100) {} 0 0 0)", color_space, color_space),
        &format!("color({} 0 0 0)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} 0 0 0 / 0)",
          color_space, color_space
        ),
        &format!("color({} 0 0 0 / 0)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} 0 y z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0 -20.5 100)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x 0 z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 0 100)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x y 0 / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 0)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x y z / 0)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100 / 0)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} 0 y z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0 -20.5 100 / 0.4)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x 0 z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 0 100 / 0.4)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x y 0 / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 0 / 0.4)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x y z / 0)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100 / 0)", result_color_space),
      );

      // Testing replacement with a constant.
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} 0.2 y z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.2 -20.5 100)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x 0.2 z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 0.2 100)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x y 0.2 / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 0.2)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x y z / 0.2)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100 / 0.2)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x y z / 20%)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100 / 0.2)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} 0.2 y z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0.2 -20.5 100 / 0.4)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x 0.2 z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 0.2 100 / 0.4)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x y 0.2 / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 0.2 / 0.4)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x y z / 0.2)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100 / 0.2)", result_color_space),
      );

      // Testing valid permutation (types match).
      test(
        &format!("color(from color({} 7 -20.5 100) {} y z x)", color_space, color_space),
        &format!("color({} -20.5 100 7)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x x x / x)",
          color_space, color_space
        ),
        &format!("color({} 7 7 7)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} y z x)",
          color_space, color_space
        ),
        &format!("color({} -20.5 100 7)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x x x / x)",
          color_space, color_space
        ),
        &format!("color({} 7 7 7)", result_color_space),
      );

      // Testing with calc().
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} calc(x) calc(y) calc(z))",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} calc(x) calc(y) calc(z) / calc(alpha))",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100 / 0.4)", result_color_space),
      );

      // Testing with 'none'.
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} none none none)",
          color_space, color_space
        ),
        &format!("color({} none none none)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} none none none / none)",
          color_space, color_space
        ),
        &format!("color({} none none none / none)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x y none)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 none)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x y none / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 none)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100) {} x y z / none)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100 / none)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x y none / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 none / 0.4)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / 40%) {} x y z / none)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100 / none)", result_color_space),
      );
      // FIXME: Clarify with spec editors if 'none' should pass through to the constants.
      test(
        &format!(
          "color(from color({} none none none) {} x y z)",
          color_space, color_space
        ),
        &format!("color({} 0 0 0)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} none none none / none) {} x y z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 0 0 0 / 0)", result_color_space),
      );
      test(
        &format!("color(from color({} 7 none 100) {} x y z)", color_space, color_space),
        &format!("color({} 7 0 100)", result_color_space),
      );
      test(
        &format!(
          "color(from color({} 7 -20.5 100 / none) {} x y z / alpha)",
          color_space, color_space
        ),
        &format!("color({} 7 -20.5 100 / 0)", result_color_space),
      );

      // https://github.com/web-platform-tests/wpt/blob/master/css/css-color/parsing/relative-color-invalid.html
      minify_test(
        ".foo{color:rgb(from rebeccapurple r 10deg 10)}",
        ".foo{color:rgb(from rebeccapurple r 10deg 10)}",
      );
      minify_test(
        ".foo{color:rgb(from rebeccapurple l g b)}",
        ".foo{color:rgb(from rebeccapurple l g b)}",
      );
      minify_test(
        ".foo{color:hsl(from rebeccapurple s h l)}",
        ".foo{color:hsl(from rebeccapurple s h l)}",
      );
      minify_test(
        ".foo{color:hsl(from rebeccapurple s s s / s)}",
        ".foo{color:hsl(from rebeccapurple s s s/s)}",
      );
      minify_test(
        ".foo{color:hsl(from rebeccapurple alpha alpha alpha / alpha)}",
        ".foo{color:hsl(from rebeccapurple alpha alpha alpha/alpha)}",
      );
    }
  }

  #[test]
  fn test_color_mix() {
    minify_test(
      ".foo { color: color-mix(in lab, purple 50%, plum 50%); }",
      ".foo{color:lab(51.5117% 43.3777 -29.0443)}",
    );
    minify_test(
      ".foo { color: color-mix(in lch, peru 40%, palegoldenrod); }",
      ".foo{color:lch(79.7255% 40.4542 84.7634)}",
    );
    minify_test(
      ".foo { color: color-mix(in lch, teal 65%, olive); }",
      ".foo{color:lch(49.4431% 40.4806 162.546)}",
    );
    minify_test(
      ".foo { color: color-mix(in lch, white, black); }",
      ".foo{color:lch(50% 0 none)}",
    );
    minify_test(
      ".foo { color: color-mix(in xyz, rgb(82.02% 30.21% 35.02%) 75.23%, rgb(5.64% 55.94% 85.31%)); }",
      ".foo{color:color(xyz .287458 .208776 .260566)}",
    );
    minify_test(
      ".foo { color: color-mix(in lch, white, blue); }",
      ".foo{color:lch(64.7842% 65.6007 301.364)}",
    );
    minify_test(
      ".foo { color: color-mix(in oklch, white, blue); }",
      ".foo{color:oklch(72.6007% .156607 264.052)}",
    );
    minify_test(
      ".foo { color: color-mix(in srgb, white, blue); }",
      ".foo{color:#8080ff}",
    );
    minify_test(
      ".foo { color: color-mix(in lch, blue, white); }",
      ".foo{color:lch(64.7842% 65.6007 301.364)}",
    );
    minify_test(
      ".foo { color: color-mix(in oklch, blue, white); }",
      ".foo{color:oklch(72.6007% .156607 264.052)}",
    );
    minify_test(
      ".foo { color: color-mix(in srgb, blue, white); }",
      ".foo{color:#8080ff}",
    );
    // minify_test(".foo { color: color-mix(in hsl, color(display-p3 0 1 0) 80%, yellow); }", ".foo{color:hsl(108 100% 49.9184%) }");
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120 100% 49.898%) 80%, yellow); }",
      ".foo{color:#33fe00}",
    );
    minify_test(
      ".foo { color: color-mix(in srgb, rgb(100% 0% 0% / 0.7) 25%, rgb(0% 100% 0% / 0.2)); }",
      ".foo{color:#89760053}",
    );
    minify_test(
      ".foo { color: color-mix(in srgb, rgb(100% 0% 0% / 0.7) 20%, rgb(0% 100% 0% / 0.2) 60%); }",
      ".foo{color:#89760042}",
    );
    minify_test(
      ".foo { color: color-mix(in lch, color(display-p3 0 1 none), color(display-p3 0 0 1)); }",
      ".foo{color:lch(58.8143% 141.732 218.684)}",
    );
    minify_test(
      ".foo { color: color-mix(in srgb, rgb(128 128 none), rgb(none none 128)); }",
      ".foo{color:gray}",
    );
    minify_test(
      ".foo { color: color-mix(in srgb, rgb(50% 50% none), rgb(none none 50%)); }",
      ".foo{color:gray}",
    );
    minify_test(
      ".foo { color: color-mix(in srgb, rgb(none 50% none), rgb(50% none 50%)); }",
      ".foo{color:gray}",
    );
    minify_test(
      ".foo { --color: color-mix(in lch, teal 65%, olive); }",
      ".foo{--color:lch(49.4431% 40.4806 162.546)}",
    );
    minify_test(
      ".foo { color: color-mix(in xyz, transparent, green 65%); }",
      ".foo{color:color(xyz .0771883 .154377 .0257295/.65)}",
    );
    prefix_test(
      ".foo { color: color-mix(in xyz, transparent, green 65%); }",
      indoc! { r#"
      .foo {
        color: #008000a6;
        color: color(xyz .0771883 .154377 .0257295 / .65);
      }
      "# },
      Browsers {
        chrome: Some(95 << 16),
        ..Default::default()
      },
    );
    minify_test(
      ".foo { color: color-mix(in srgb, currentColor, blue); }",
      ".foo{color:color-mix(in srgb,currentColor,blue)}",
    );
    minify_test(
      ".foo { color: color-mix(in srgb, blue, currentColor); }",
      ".foo{color:color-mix(in srgb,blue,currentColor)}",
    );
    minify_test(
      ".foo { color: color-mix(in srgb, accentcolor, blue); }",
      ".foo{color:color-mix(in srgb,accentcolor,blue)}",
    );
    minify_test(
      ".foo { color: color-mix(in srgb, blue, accentcolor); }",
      ".foo{color:color-mix(in srgb,blue,accentcolor)}",
    );

    // regex for converting web platform tests:
    // test_computed_value\(.*?, `(.*?)`, `(.*?)`\);
    // minify_test(".foo { color: $1 }", ".foo{color:$2}");

    // https://github.com/web-platform-tests/wpt/blob/f8c76b11cff66a7adc87264a18e39353cb5a60c9/css/css-color/parsing/color-mix-computed.html
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20%), hsl(30deg 30% 40%)) }",
      ".foo{color:#545c3d}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20%) 25%, hsl(30deg 30% 40%)) }",
      ".foo{color:#706a43}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, 25% hsl(120deg 10% 20%), hsl(30deg 30% 40%)) }",
      ".foo{color:#706a43}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20%), 25% hsl(30deg 30% 40%)) }",
      ".foo{color:#3d4936}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20%), hsl(30deg 30% 40%) 25%) }",
      ".foo{color:#3d4936}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20%) 25%, hsl(30deg 30% 40%) 75%) }",
      ".foo{color:#706a43}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20%) 30%, hsl(30deg 30% 40%) 90%) }",
      ".foo{color:#706a43}",
    ); // Scale down > 100% sum.
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20%) 12.5%, hsl(30deg 30% 40%) 37.5%) }",
      ".foo{color:#706a4380}",
    ); // Scale up < 100% sum, causes alpha multiplication.
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20%) 0%, hsl(30deg 30% 40%)) }",
      ".foo{color:#856647}",
    );

    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20% / .4), hsl(30deg 30% 40% / .8)) }",
      ".foo{color:#5f694199}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20%) 25%, hsl(30deg 30% 40% / .8)) }",
      ".foo{color:#6c6742d9}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, 25% hsl(120deg 10% 20% / .4), hsl(30deg 30% 40% / .8)) }",
      ".foo{color:#797245b3}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20% / .4), 25% hsl(30deg 30% 40% / .8)) }",
      ".foo{color:#44543b80}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20% / .4), hsl(30deg 30% 40% / .8) 25%) }",
      ".foo{color:#44543b80}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20% / .4) 25%, hsl(30deg 30% 40% / .8) 75%) }",
      ".foo{color:#797245b3}",
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20% / .4) 30%, hsl(30deg 30% 40% / .8) 90%) }",
      ".foo{color:#797245b3}",
    ); // Scale down > 100% sum.
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20% / .4) 12.5%, hsl(30deg 30% 40% / .8) 37.5%) }",
      ".foo{color:#79724559}",
    ); // Scale up < 100% sum, causes alpha multiplication.
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 10% 20% / .4) 0%, hsl(30deg 30% 40% / .8)) }",
      ".foo{color:#856647cc}",
    );

    fn canonicalize(s: &str) -> String {
      use crate::traits::{Parse, ToCss};
      use crate::values::color::CssColor;
      use cssparser::{Parser, ParserInput};

      let mut input = ParserInput::new(s);
      let mut parser = Parser::new(&mut input);
      let v = CssColor::parse(&mut parser).unwrap().to_rgb().unwrap();
      format!(".foo{{color:{}}}", v.to_css_string(PrinterOptions::default()).unwrap())
    }

    // regex for converting web platform tests:
    // test_computed_value\(.*?, `(.*?)`, canonicalize\(`(.*?)`\)\);
    // minify_test(".foo { color: $1 }", &canonicalize("$2"));

    minify_test(
      ".foo { color: color-mix(in hsl, hsl(40deg 50% 50%), hsl(60deg 50% 50%)) }",
      &canonicalize("hsl(50deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(60deg 50% 50%), hsl(40deg 50% 50%)) }",
      &canonicalize("hsl(50deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(50deg 50% 50%), hsl(330deg 50% 50%)) }",
      &canonicalize("hsl(10deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(330deg 50% 50%), hsl(50deg 50% 50%)) }",
      &canonicalize("hsl(10deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(20deg 50% 50%), hsl(320deg 50% 50%)) }",
      &canonicalize("hsl(350deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(320deg 50% 50%), hsl(20deg 50% 50%)) }",
      &canonicalize("hsl(350deg 50% 50%)"),
    );

    minify_test(
      ".foo { color: color-mix(in hsl shorter hue, hsl(40deg 50% 50%), hsl(60deg 50% 50%)) }",
      &canonicalize("hsl(50deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl shorter hue, hsl(60deg 50% 50%), hsl(40deg 50% 50%)) }",
      &canonicalize("hsl(50deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl shorter hue, hsl(50deg 50% 50%), hsl(330deg 50% 50%)) }",
      &canonicalize("hsl(10deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl shorter hue, hsl(330deg 50% 50%), hsl(50deg 50% 50%)) }",
      &canonicalize("hsl(10deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl shorter hue, hsl(20deg 50% 50%), hsl(320deg 50% 50%)) }",
      &canonicalize("hsl(350deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl shorter hue, hsl(320deg 50% 50%), hsl(20deg 50% 50%)) }",
      &canonicalize("hsl(350deg 50% 50%)"),
    );

    minify_test(
      ".foo { color: color-mix(in hsl longer hue, hsl(40deg 50% 50%), hsl(60deg 50% 50%)) }",
      &canonicalize("hsl(230deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl longer hue, hsl(60deg 50% 50%), hsl(40deg 50% 50%)) }",
      &canonicalize("hsl(230deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl longer hue, hsl(50deg 50% 50%), hsl(330deg 50% 50%)) }",
      &canonicalize("hsl(190deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl longer hue, hsl(330deg 50% 50%), hsl(50deg 50% 50%)) }",
      &canonicalize("hsl(190deg 50% 50%)"),
    );
    // minify_test(".foo { color: color-mix(in hsl longer hue, hsl(20deg 50% 50%), hsl(320deg 50% 50%)) }", &canonicalize("hsl(170deg 50% 50%)"));
    // minify_test(".foo { color: color-mix(in hsl longer hue, hsl(320deg 50% 50%), hsl(20deg 50% 50%)) }", &canonicalize("hsl(170deg 50% 50%)"));

    minify_test(
      ".foo { color: color-mix(in hsl increasing hue, hsl(40deg 50% 50%), hsl(60deg 50% 50%)) }",
      &canonicalize("hsl(50deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl increasing hue, hsl(60deg 50% 50%), hsl(40deg 50% 50%)) }",
      &canonicalize("hsl(230deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl increasing hue, hsl(50deg 50% 50%), hsl(330deg 50% 50%)) }",
      &canonicalize("hsl(190deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl increasing hue, hsl(330deg 50% 50%), hsl(50deg 50% 50%)) }",
      &canonicalize("hsl(10deg 50% 50%)"),
    );
    // minify_test(".foo { color: color-mix(in hsl increasing hue, hsl(20deg 50% 50%), hsl(320deg 50% 50%)) }", &canonicalize("hsl(170deg 50% 50%)"));
    // minify_test(".foo { color: color-mix(in hsl increasing hue, hsl(320deg 50% 50%), hsl(20deg 50% 50%)) }", &canonicalize("hsl(350deg 50% 50%)"));

    minify_test(
      ".foo { color: color-mix(in hsl decreasing hue, hsl(40deg 50% 50%), hsl(60deg 50% 50%)) }",
      &canonicalize("hsl(230deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl decreasing hue, hsl(60deg 50% 50%), hsl(40deg 50% 50%)) }",
      &canonicalize("hsl(50deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl decreasing hue, hsl(50deg 50% 50%), hsl(330deg 50% 50%)) }",
      &canonicalize("hsl(10deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl decreasing hue, hsl(330deg 50% 50%), hsl(50deg 50% 50%)) }",
      &canonicalize("hsl(190deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl decreasing hue, hsl(20deg 50% 50%), hsl(320deg 50% 50%)) }",
      &canonicalize("hsl(350deg 50% 50%)"),
    );
    // minify_test(".foo { color: color-mix(in hsl decreasing hue, hsl(320deg 50% 50%), hsl(20deg 50% 50%)) }", &canonicalize("hsl(170deg 50% 50%)"));

    minify_test(
      ".foo { color: color-mix(in hsl specified hue, hsl(40deg 50% 50%), hsl(60deg 50% 50%)) }",
      &canonicalize("hsl(50deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl specified hue, hsl(60deg 50% 50%), hsl(40deg 50% 50%)) }",
      &canonicalize("hsl(50deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl specified hue, hsl(50deg 50% 50%), hsl(330deg 50% 50%)) }",
      &canonicalize("hsl(190deg 50% 50%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl specified hue, hsl(330deg 50% 50%), hsl(50deg 50% 50%)) }",
      &canonicalize("hsl(190deg 50% 50%)"),
    );
    // minify_test(".foo { color: color-mix(in hsl specified hue, hsl(20deg 50% 50%), hsl(320deg 50% 50%)) }", &canonicalize("hsl(170deg 50% 50%)"));
    // minify_test(".foo { color: color-mix(in hsl specified hue, hsl(320deg 50% 50%), hsl(20deg 50% 50%)) }", &canonicalize("hsl(170deg 50% 50%)"));

    minify_test(
      ".foo { color: color-mix(in hsl, hsl(none none none), hsl(none none none)) }",
      &canonicalize("hsl(none none none)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(none none none), hsl(30deg 40% 80%)) }",
      &canonicalize("hsl(30deg 40% 80%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 20% 40%), hsl(none none none)) }",
      &canonicalize("hsl(120deg 20% 40%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 20% none), hsl(30deg 40% 60%)) }",
      &canonicalize("hsl(75deg 30% 60%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(120deg 20% 40%), hsl(30deg 20% none)) }",
      &canonicalize("hsl(75deg 20% 40%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hsl, hsl(none 20% 40%), hsl(30deg none 80%)) }",
      &canonicalize("hsl(30deg 20% 60%)"),
    );

    minify_test(
      ".foo { color: color-mix(in hsl, color(display-p3 0 1 0) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(0, 249, 66)"),
    ); // Naive clip based mapping would give rgb(0, 255, 0).
    minify_test(
      ".foo { color: color-mix(in hsl, lab(100% 104.3 -50.9) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(255, 255, 255)"),
    ); // Naive clip based mapping would give rgb(255, 150, 255).
    minify_test(
      ".foo { color: color-mix(in hsl, lab(0% 104.3 -50.9) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(42, 0, 34)"),
    ); // Naive clip based mapping would give rgb(90, 0, 76). NOTE: 0% lightness in Lab/LCH does not automatically correspond with sRGB black,
    minify_test(
      ".foo { color: color-mix(in hsl, lch(100% 116 334) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(255, 255, 255)"),
    ); // Naive clip based mapping would give rgb(255, 150, 255).
    minify_test(
      ".foo { color: color-mix(in hsl, lch(0% 116 334) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(42, 0, 34)"),
    ); // Naive clip based mapping would give rgb(90, 0, 76). NOTE: 0% lightness in Lab/LCH does not automatically correspond with sRGB black,
    minify_test(
      ".foo { color: color-mix(in hsl, oklab(100% 0.365 -0.16) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(255, 255, 255)"),
    ); // Naive clip based mapping would give rgb(255, 92, 255).
    minify_test(
      ".foo { color: color-mix(in hsl, oklab(0% 0.365 -0.16) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(0, 0, 0)"),
    ); // Naive clip based mapping would give rgb(19, 0, 24).
    minify_test(
      ".foo { color: color-mix(in hsl, oklch(100% 0.399 336.3) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(255, 255, 255)"),
    ); // Naive clip based mapping would give rgb(255, 91, 255).
    minify_test(
      ".foo { color: color-mix(in hsl, oklch(0% 0.399 336.3) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(0, 0, 0)"),
    ); // Naive clip based mapping would give rgb(20, 0, 24).

    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20%), hwb(30deg 30% 40%)) }",
      &canonicalize("rgb(147, 179, 52)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20%) 25%, hwb(30deg 30% 40%)) }",
      &canonicalize("rgb(166, 153, 64)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, 25% hwb(120deg 10% 20%), hwb(30deg 30% 40%)) }",
      &canonicalize("rgb(166, 153, 64)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20%), 25% hwb(30deg 30% 40%)) }",
      &canonicalize("rgb(96, 191, 39)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20%), hwb(30deg 30% 40%) 25%) }",
      &canonicalize("rgb(96, 191, 39)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20%) 25%, hwb(30deg 30% 40%) 75%) }",
      &canonicalize("rgb(166, 153, 64)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20%) 30%, hwb(30deg 30% 40%) 90%) }",
      &canonicalize("rgb(166, 153, 64)"),
    ); // Scale down > 100% sum.
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20%) 12.5%, hwb(30deg 30% 40%) 37.5%) }",
      &canonicalize("rgba(166, 153, 64, 0.5)"),
    ); // Scale up < 100% sum, causes alpha multiplication.
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20%) 0%, hwb(30deg 30% 40%)) }",
      &canonicalize("rgb(153, 115, 77)"),
    );

    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20% / .4), hwb(30deg 30% 40% / .8)) }",
      &canonicalize("rgba(143, 170, 60, 0.6)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20% / .4) 25%, hwb(30deg 30% 40% / .8)) }",
      &canonicalize("rgba(160, 149, 70, 0.7)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, 25% hwb(120deg 10% 20% / .4), hwb(30deg 30% 40% / .8)) }",
      &canonicalize("rgba(160, 149, 70, 0.7)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20%), 25% hwb(30deg 30% 40% / .8)) }",
      &canonicalize("rgba(95, 193, 37, 0.95)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20% / .4), hwb(30deg 30% 40% / .8) 25%) }",
      &canonicalize("rgba(98, 184, 46, 0.5)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20% / .4) 25%, hwb(30deg 30% 40% / .8) 75%) }",
      &canonicalize("rgba(160, 149, 70, 0.7)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20% / .4) 30%, hwb(30deg 30% 40% / .8) 90%) }",
      &canonicalize("rgba(160, 149, 70, 0.7)"),
    ); // Scale down > 100% sum.
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20% / .4) 12.5%, hwb(30deg 30% 40% / .8) 37.5%) }",
      &canonicalize("rgba(160, 149, 70, 0.35)"),
    ); // Scale up < 100% sum, causes alpha multiplication.
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(120deg 10% 20% / .4) 0%, hwb(30deg 30% 40% / .8)) }",
      &canonicalize("rgba(153, 115, 77, 0.8)"),
    );

    //  minify_test(".foo { color: color-mix(in hwb, hwb(40deg 30% 40%), hwb(60deg 30% 40%)) }", &canonicalize("hwb(50deg 30% 40%)"));
    //  minify_test(".foo { color: color-mix(in hwb, hwb(60deg 30% 40%), hwb(40deg 30% 40%)) }", &canonicalize("hwb(50deg 30% 40%)"));
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(50deg 30% 40%), hwb(330deg 30% 40%)) }",
      &canonicalize("hwb(10deg 30% 40%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(330deg 30% 40%), hwb(50deg 30% 40%)) }",
      &canonicalize("hwb(10deg 30% 40%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(20deg 30% 40%), hwb(320deg 30% 40%)) }",
      &canonicalize("hwb(350deg 30% 40%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb, hwb(320deg 30% 40%), hwb(20deg 30% 40%)) }",
      &canonicalize("hwb(350deg 30% 40%)"),
    );

    //  minify_test(".foo { color: color-mix(in hwb shorter hue, hwb(40deg 30% 40%), hwb(60deg 30% 40%)) }", &canonicalize("hwb(50deg 30% 40%)"));
    //  minify_test(".foo { color: color-mix(in hwb shorter hue, hwb(60deg 30% 40%), hwb(40deg 30% 40%)) }", &canonicalize("hwb(50deg 30% 40%)"));
    minify_test(
      ".foo { color: color-mix(in hwb shorter hue, hwb(50deg 30% 40%), hwb(330deg 30% 40%)) }",
      &canonicalize("hwb(10deg 30% 40%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb shorter hue, hwb(330deg 30% 40%), hwb(50deg 30% 40%)) }",
      &canonicalize("hwb(10deg 30% 40%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb shorter hue, hwb(20deg 30% 40%), hwb(320deg 30% 40%)) }",
      &canonicalize("hwb(350deg 30% 40%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb shorter hue, hwb(320deg 30% 40%), hwb(20deg 30% 40%)) }",
      &canonicalize("hwb(350deg 30% 40%)"),
    );

    minify_test(
      ".foo { color: color-mix(in hwb longer hue, hwb(40deg 30% 40%), hwb(60deg 30% 40%)) }",
      &canonicalize("hwb(230deg 30% 40%)"),
    );
    minify_test(
      ".foo { color: color-mix(in hwb longer hue, hwb(60deg 30% 40%), hwb(40deg 30% 40%)) }",
      &canonicalize("hwb(230deg 30% 40%)"),
    );
    //  minify_test(".foo { color: color-mix(in hwb longer hue, hwb(50deg 30% 40%), hwb(330deg 30% 40%)) }", &canonicalize("hwb(190deg 30% 40%)"));
    //  minify_test(".foo { color: color-mix(in hwb longer hue, hwb(330deg 30% 40%), hwb(50deg 30% 40%)) }", &canonicalize("hwb(190deg 30% 40%)"));
    //  minify_test(".foo { color: color-mix(in hwb longer hue, hwb(20deg 30% 40%), hwb(320deg 30% 40%)) }", &canonicalize("hwb(170deg 30% 40%)"));
    //  minify_test(".foo { color: color-mix(in hwb longer hue, hwb(320deg 30% 40%), hwb(20deg 30% 40%)) }", &canonicalize("hwb(170deg 30% 40%)"));

    // minify_test(".foo { color: color-mix(in hwb increasing hue, hwb(40deg 30% 40%), hwb(60deg 30% 40%)) }", &canonicalize("hwb(50deg 30% 40%)"));
    minify_test(
      ".foo { color: color-mix(in hwb increasing hue, hwb(60deg 30% 40%), hwb(40deg 30% 40%)) }",
      &canonicalize("hwb(230deg 30% 40%)"),
    );
    // minify_test(".foo { color: color-mix(in hwb increasing hue, hwb(50deg 30% 40%), hwb(330deg 30% 40%)) }", &canonicalize("hwb(190deg 30% 40%)"));
    minify_test(
      ".foo { color: color-mix(in hwb increasing hue, hwb(330deg 30% 40%), hwb(50deg 30% 40%)) }",
      &canonicalize("hwb(10deg 30% 40%)"),
    );
    // minify_test(".foo { color: color-mix(in hwb increasing hue, hwb(20deg 30% 40%), hwb(320deg 30% 40%)) }", &canonicalize("hwb(170deg 30% 40%)"));
    minify_test(
      ".foo { color: color-mix(in hwb increasing hue, hwb(320deg 30% 40%), hwb(20deg 30% 40%)) }",
      &canonicalize("hwb(350deg 30% 40%)"),
    );

    minify_test(
      ".foo { color: color-mix(in hwb decreasing hue, hwb(40deg 30% 40%), hwb(60deg 30% 40%)) }",
      &canonicalize("hwb(230deg 30% 40%)"),
    );
    // minify_test(".foo { color: color-mix(in hwb decreasing hue, hwb(60deg 30% 40%), hwb(40deg 30% 40%)) }", &canonicalize("hwb(50deg 30% 40%)"));
    minify_test(
      ".foo { color: color-mix(in hwb decreasing hue, hwb(50deg 30% 40%), hwb(330deg 30% 40%)) }",
      &canonicalize("hwb(10deg 30% 40%)"),
    );
    // minify_test(".foo { color: color-mix(in hwb decreasing hue, hwb(330deg 30% 40%), hwb(50deg 30% 40%)) }", &canonicalize("hwb(190deg 30% 40%)"));
    minify_test(
      ".foo { color: color-mix(in hwb decreasing hue, hwb(20deg 30% 40%), hwb(320deg 30% 40%)) }",
      &canonicalize("hwb(350deg 30% 40%)"),
    );
    // minify_test(".foo { color: color-mix(in hwb decreasing hue, hwb(320deg 30% 40%), hwb(20deg 30% 40%)) }", &canonicalize("hwb(170deg 30% 40%)"));

    // minify_test(".foo { color: color-mix(in hwb specified hue, hwb(40deg 30% 40%), hwb(60deg 30% 40%)) }", &canonicalize("hwb(50deg 30% 40%)"));
    // minify_test(".foo { color: color-mix(in hwb specified hue, hwb(60deg 30% 40%), hwb(40deg 30% 40%)) }", &canonicalize("hwb(50deg 30% 40%)"));
    // minify_test(".foo { color: color-mix(in hwb specified hue, hwb(50deg 30% 40%), hwb(330deg 30% 40%)) }", &canonicalize("hwb(190deg 30% 40%)"));
    // minify_test(".foo { color: color-mix(in hwb specified hue, hwb(330deg 30% 40%), hwb(50deg 30% 40%)) }", &canonicalize("hwb(190deg 30% 40%)"));
    // minify_test(".foo { color: color-mix(in hwb specified hue, hwb(20deg 30% 40%), hwb(320deg 30% 40%)) }", &canonicalize("hwb(170deg 30% 40%)"));
    // minify_test(".foo { color: color-mix(in hwb specified hue, hwb(320deg 30% 40%), hwb(20deg 30% 40%)) }", &canonicalize("hwb(170deg 30% 40%)"));

    minify_test(
      ".foo { color: color-mix(in hwb, color(display-p3 0 1 0) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(0, 249, 66)"),
    ); // Naive clip based mapping would give rgb(0, 255, 0).
    minify_test(
      ".foo { color: color-mix(in hwb, lab(100% 104.3 -50.9) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(255, 255, 255)"),
    ); // Naive clip based mapping would give rgb(255, 150, 255).
    minify_test(
      ".foo { color: color-mix(in hwb, lab(0% 104.3 -50.9) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(42, 0, 34)"),
    ); // Naive clip based mapping would give rgb(90, 0, 76). NOTE: 0% lightness in Lab/LCH does not automatically correspond with sRGB black,
    minify_test(
      ".foo { color: color-mix(in hwb, lch(100% 116 334) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(255, 255, 255)"),
    ); // Naive clip based mapping would give rgb(255, 150, 255).
    minify_test(
      ".foo { color: color-mix(in hwb, lch(0% 116 334) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(42, 0, 34)"),
    ); // Naive clip based mapping would give rgb(90, 0, 76). NOTE: 0% lightness in Lab/LCH does not automatically correspond with sRGB black,
    minify_test(
      ".foo { color: color-mix(in hwb, oklab(100% 0.365 -0.16) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(255, 255, 255)"),
    ); // Naive clip based mapping would give rgb(255, 92, 255).
    minify_test(
      ".foo { color: color-mix(in hwb, oklab(0% 0.365 -0.16) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(0, 0, 0)"),
    ); // Naive clip based mapping would give rgb(19, 0, 24).
    minify_test(
      ".foo { color: color-mix(in hwb, oklch(100% 0.399 336.3) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(255, 255, 255)"),
    ); // Naive clip based mapping would give rgb(255, 91, 255).
    minify_test(
      ".foo { color: color-mix(in hwb, oklch(0% 0.399 336.3) 100%, rgb(0, 0, 0) 0%) }",
      &canonicalize("rgb(0, 0, 0)"),
    ); // Naive clip based mapping would give rgb(20, 0, 24).

    for color_space in &["lch", "oklch"] {
      // regex for converting web platform tests:
      // test_computed_value\(.*?, `color-mix\(in \$\{colorSpace\}(.*?), (.*?)\$\{colorSpace\}(.*?) \$\{colorSpace\}(.*?)`, `\$\{colorSpace\}(.*?)`\);
      // minify_test(&format!(".foo {{ color: color-mix(in {0}$1, $2{0}$3 {0}$4 }}", color_space), &format!(".foo{{color:{}$5}}", color_space));

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg), {0}(50% 60 70deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg) 25%, {0}(50% 60 70deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(40% 50 60)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, 25% {0}(10% 20 30deg), {0}(50% 60 70deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(40% 50 60)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg), 25% {0}(50% 60 70deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(20% 30 40)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg), {0}(50% 60 70deg) 25%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(20% 30 40)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg) 25%, {0}(50% 60 70deg) 75%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(40% 50 60)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg) 30%, {0}(50% 60 70deg) 90%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(40% 50 60)}}", color_space),
      ); // Scale down > 100% sum.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg) 12.5%, {0}(50% 60 70deg) 37.5%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(40% 50 60/.5)}}", color_space),
      ); // Scale up < 100% sum, causes alpha multiplication.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg) 0%, {0}(50% 60 70deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(50% 60 70)}}", color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / .4), {0}(50% 60 70deg / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(36.6667% 46.6667 50/.6)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / .4) 25%, {0}(50% 60 70deg / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(44.2857% 54.2857 60/.7)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, 25% {0}(10% 20 30deg / .4), {0}(50% 60 70deg / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(44.2857% 54.2857 60/.7)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / .4), 25% {0}(50% 60 70deg / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(26% 36 40/.5)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / .4), {0}(50% 60 70deg / .8) 25%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(26% 36 40/.5)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / .4) 25%, {0}(50% 60 70deg / .8) 75%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(44.2857% 54.2857 60/.7)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / .4) 30%, {0}(50% 60 70deg / .8) 90%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(44.2857% 54.2857 60/.7)}}", color_space),
      ); // Scale down > 100% sum.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / .4) 12.5%, {0}(50% 60 70deg / .8) 37.5%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(44.2857% 54.2857 60/.35)}}", color_space),
      ); // Scale up < 100% sum, causes alpha multiplication.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / .4) 0%, {0}(50% 60 70deg / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(50% 60 70/.8)}}", color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(100% 0 40deg), {0}(100% 0 60deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(100% 0 60deg), {0}(100% 0 40deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(100% 0 50deg), {0}(100% 0 330deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 10)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(100% 0 330deg), {0}(100% 0 50deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 10)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(100% 0 20deg), {0}(100% 0 320deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 350)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(100% 0 320deg), {0}(100% 0 20deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 350)}}", color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} shorter hue, {0}(100% 0 40deg), {0}(100% 0 60deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} shorter hue, {0}(100% 0 60deg), {0}(100% 0 40deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} shorter hue, {0}(100% 0 50deg), {0}(100% 0 330deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 10)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} shorter hue, {0}(100% 0 330deg), {0}(100% 0 50deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 10)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} shorter hue, {0}(100% 0 20deg), {0}(100% 0 320deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 350)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} shorter hue, {0}(100% 0 320deg), {0}(100% 0 20deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 350)}}", color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} longer hue, {0}(100% 0 40deg), {0}(100% 0 60deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 230)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} longer hue, {0}(100% 0 60deg), {0}(100% 0 40deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 230)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} longer hue, {0}(100% 0 50deg), {0}(100% 0 330deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 190)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} longer hue, {0}(100% 0 330deg), {0}(100% 0 50deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 190)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} longer hue, {0}(100% 0 20deg), {0}(100% 0 320deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 170)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} longer hue, {0}(100% 0 320deg), {0}(100% 0 20deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 170)}}", color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} increasing hue, {0}(100% 0 40deg), {0}(100% 0 60deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} increasing hue, {0}(100% 0 60deg), {0}(100% 0 40deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 230)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} increasing hue, {0}(100% 0 50deg), {0}(100% 0 330deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 190)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} increasing hue, {0}(100% 0 330deg), {0}(100% 0 50deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 10)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} increasing hue, {0}(100% 0 20deg), {0}(100% 0 320deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 170)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} increasing hue, {0}(100% 0 320deg), {0}(100% 0 20deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 350)}}", color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} decreasing hue, {0}(100% 0 40deg), {0}(100% 0 60deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 230)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} decreasing hue, {0}(100% 0 60deg), {0}(100% 0 40deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} decreasing hue, {0}(100% 0 50deg), {0}(100% 0 330deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 10)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} decreasing hue, {0}(100% 0 330deg), {0}(100% 0 50deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 190)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} decreasing hue, {0}(100% 0 20deg), {0}(100% 0 320deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 350)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} decreasing hue, {0}(100% 0 320deg), {0}(100% 0 20deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 170)}}", color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} specified hue, {0}(100% 0 40deg), {0}(100% 0 60deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} specified hue, {0}(100% 0 60deg), {0}(100% 0 40deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} specified hue, {0}(100% 0 50deg), {0}(100% 0 330deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 190)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} specified hue, {0}(100% 0 330deg), {0}(100% 0 50deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 190)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} specified hue, {0}(100% 0 20deg), {0}(100% 0 320deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 170)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0} specified hue, {0}(100% 0 320deg), {0}(100% 0 20deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(100% 0 170)}}", color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(none none none), {0}(none none none)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(none none none)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(none none none), {0}(50% 60 70deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(50% 60 70)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg), {0}(none none none)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(10% 20 30)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 none), {0}(50% 60 70deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 70)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg), {0}(50% 60 none)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 30)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(none 20 30deg), {0}(50% none 70deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(50% 20 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / none), {0}(50% 60 70deg)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / none), {0}(50% 60 70deg / 0.5)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 50/.5)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30deg / none), {0}(50% 60 70deg / none)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 50/none)}}", color_space),
      );
    }

    for color_space in ["lab", "oklab"] {
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30), {0}(50% 60 70)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30) 25%, {0}(50% 60 70)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(40% 50 60)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, 25% {0}(10% 20 30), {0}(50% 60 70)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(40% 50 60)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30), 25% {0}(50% 60 70)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(20% 30 40)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30), {0}(50% 60 70) 25%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(20% 30 40)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30) 25%, {0}(50% 60 70) 75%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(40% 50 60)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30) 30%, {0}(50% 60 70) 90%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(40% 50 60)}}", color_space),
      ); // Scale down > 100% sum.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30) 12.5%, {0}(50% 60 70) 37.5%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(40% 50 60/.5)}}", color_space),
      ); // Scale up < 100% sum, causes alpha multiplication.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30) 0%, {0}(50% 60 70)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(50% 60 70)}}", color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / .4), {0}(50% 60 70 / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(36.6667% 46.6667 56.6667/.6)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / .4) 25%, {0}(50% 60 70 / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(44.2857% 54.2857 64.2857/.7)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, 25% {0}(10% 20 30 / .4), {0}(50% 60 70 / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(44.2857% 54.2857 64.2857/.7)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / .4), 25% {0}(50% 60 70 / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(26% 36 46/.5)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / .4), {0}(50% 60 70 / .8) 25%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(26% 36 46/.5)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / .4) 25%, {0}(50% 60 70 / .8) 75%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(44.2857% 54.2857 64.2857/.7)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / .4) 30%, {0}(50% 60 70 / .8) 90%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(44.2857% 54.2857 64.2857/.7)}}", color_space),
      ); // Scale down > 100% sum.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / .4) 12.5%, {0}(50% 60 70 / .8) 37.5%) }}",
          color_space
        ),
        &format!(".foo{{color:{}(44.2857% 54.2857 64.2857/.35)}}", color_space),
      ); // Scale up < 100% sum, causes alpha multiplication.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / .4) 0%, {0}(50% 60 70 / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(50% 60 70/.8)}}", color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(none none none), {0}(none none none)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(none none none)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(none none none), {0}(50% 60 70)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(50% 60 70)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30), {0}(none none none)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(10% 20 30)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 none), {0}(50% 60 70)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 70)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30), {0}(50% 60 none)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 30)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(none 20 30), {0}(50% none 70)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(50% 20 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / none), {0}(50% 60 70)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 50)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / none), {0}(50% 60 70 / 0.5)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 50/.5)}}", color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, {0}(10% 20 30 / none), {0}(50% 60 70 / none)) }}",
          color_space
        ),
        &format!(".foo{{color:{}(30% 40 50/none)}}", color_space),
      );
    }

    for color_space in [/*"srgb", */ "srgb-linear", "xyz", "xyz-d50", "xyz-d65"] {
      // regex for converting web platform tests:
      // test_computed_value\(.*?, `color-mix\(in \$\{colorSpace\}(.*?), (.*?)color\(\$\{colorSpace\}(.*?) color\(\$\{colorSpace\}(.*?)`, `color\(\$\{resultColorSpace\}(.*?)`\);
      // minify_test(&format!(".foo {{ color: color-mix(in {0}$1, $2color({0}$3 color({0}$4 }}", color_space), &format!(".foo{{color:color({}$5}}", result_color_space));

      let result_color_space = if color_space == "xyz-d65" { "xyz" } else { color_space };

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3), color({0} .5 .6 .7)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .3 .4 .5)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3) 25%, color({0} .5 .6 .7)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .4 .5 .6)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, 25% color({0} .1 .2 .3), color({0} .5 .6 .7)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .4 .5 .6)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3), color({0} .5 .6 .7) 25%) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .2 .3 .4)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3), 25% color({0} .5 .6 .7)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .2 .3 .4)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3) 25%, color({0} .5 .6 .7) 75%) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .4 .5 .6)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3) 30%, color({0} .5 .6 .7) 90%) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .4 .5 .6)}}", result_color_space),
      ); // Scale down > 100% sum.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3) 12.5%, color({0} .5 .6 .7) 37.5%) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .4 .5 .6/.5)}}", result_color_space),
      ); // Scale up < 100% sum, causes alpha multiplication.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3) 0%, color({0} .5 .6 .7)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .5 .6 .7)}}", result_color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / .5), color({0} .5 .6 .7 / .8)) }}",
          color_space
        ),
        &format!(
          ".foo{{color:color({} .346154 .446154 .546154/.65)}}",
          result_color_space
        ),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / .4) 25%, color({0} .5 .6 .7 / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .442857 .542857 .642857/.7)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, 25% color({0} .1 .2 .3 / .4), color({0} .5 .6 .7 / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .442857 .542857 .642857/.7)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / .4), color({0} .5 .6 .7 / .8) 25%) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .26 .36 .46/.5)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / .4), 25% color({0} .5 .6 .7 / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .26 .36 .46/.5)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / .4) 25%, color({0} .5 .6 .7 / .8) 75%) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .442857 .542857 .642857/.7)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / .4) 30%, color({0} .5 .6 .7 / .8) 90%) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .442857 .542857 .642857/.7)}}", result_color_space),
      ); // Scale down > 100% sum.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / .4) 12.5%, color({0} .5 .6 .7 / .8) 37.5%) }}",
          color_space
        ),
        &format!(
          ".foo{{color:color({} .442857 .542857 .642857/.35)}}",
          result_color_space
        ),
      ); // Scale up < 100% sum, causes alpha multiplication.
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / .4) 0%, color({0} .5 .6 .7 / .8)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .5 .6 .7/.8)}}", result_color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} 2 3 4 / 5), color({0} 4 6 8 / 10)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} 3 4.5 6)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} -2 -3 -4), color({0} -4 -6 -8)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} -3 -4.5 -6)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} -2 -3 -4 / -5), color({0} -4 -6 -8 / -10)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} 0 0 0/0)}}", result_color_space),
      );

      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} none none none), color({0} none none none)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} none none none)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} none none none), color({0} .5 .6 .7)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .5 .6 .7)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3), color({0} none none none)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .1 .2 .3)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 none), color({0} .5 .6 .7)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .3 .4 .7)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3), color({0} .5 .6 none)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .3 .4 .3)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} none .2 .3), color({0} .5 none .7)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .5 .2 .5)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / none), color({0} .5 .6 .7)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .3 .4 .5)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / none), color({0} .5 .6 .7 / 0.5)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .3 .4 .5/.5)}}", result_color_space),
      );
      minify_test(
        &format!(
          ".foo {{ color: color-mix(in {0}, color({0} .1 .2 .3 / none), color({0} .5 .6 .7 / none)) }}",
          color_space
        ),
        &format!(".foo{{color:color({} .3 .4 .5/none)}}", result_color_space),
      );
    }
  }

  #[cfg(feature = "grid")]
  #[test]
  fn test_grid() {
    minify_test(
      ".foo { grid-template-columns: [first nav-start]  150px [main-start] 1fr [last]; }",
      ".foo{grid-template-columns:[first nav-start]150px[main-start]1fr[last]}",
    );
    minify_test(
      ".foo { grid-template-columns: 150px 1fr; }",
      ".foo{grid-template-columns:150px 1fr}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(4, 1fr); }",
      ".foo{grid-template-columns:repeat(4,1fr)}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(2, [e] 40px); }",
      ".foo{grid-template-columns:repeat(2,[e]40px)}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(4, [col-start] 250px [col-end]); }",
      ".foo{grid-template-columns:repeat(4,[col-start]250px[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(4, [col-start] 60% [col-end]); }",
      ".foo{grid-template-columns:repeat(4,[col-start]60%[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(4, [col-start] 1fr [col-end]); }",
      ".foo{grid-template-columns:repeat(4,[col-start]1fr[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(4, [col-start] min-content [col-end]); }",
      ".foo{grid-template-columns:repeat(4,[col-start]min-content[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(4, [col-start] max-content [col-end]); }",
      ".foo{grid-template-columns:repeat(4,[col-start]max-content[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(4, [col-start] auto [col-end]); }",
      ".foo{grid-template-columns:repeat(4,[col-start]auto[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(4, [col-start] minmax(100px, 1fr) [col-end]); }",
      ".foo{grid-template-columns:repeat(4,[col-start]minmax(100px,1fr)[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(4, [col-start] fit-content(200px) [col-end]); }",
      ".foo{grid-template-columns:repeat(4,[col-start]fit-content(200px)[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(4, 10px [col-start] 30% [col-middle] auto [col-end]); }",
      ".foo{grid-template-columns:repeat(4,10px[col-start]30%[col-middle]auto[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(5, auto); }",
      ".foo{grid-template-columns:repeat(5,auto)}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(auto-fill, 250px); }",
      ".foo{grid-template-columns:repeat(auto-fill,250px)}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(auto-fit, 250px); }",
      ".foo{grid-template-columns:repeat(auto-fit,250px)}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(auto-fill, [col-start] 250px [col-end]); }",
      ".foo{grid-template-columns:repeat(auto-fill,[col-start]250px[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: repeat(auto-fill, [col-start] minmax(100px, 1fr) [col-end]); }",
      ".foo{grid-template-columns:repeat(auto-fill,[col-start]minmax(100px,1fr)[col-end])}",
    );
    minify_test(
      ".foo { grid-template-columns: minmax(min-content, 1fr); }",
      ".foo{grid-template-columns:minmax(min-content,1fr)}",
    );
    minify_test(
      ".foo { grid-template-columns: 200px repeat(auto-fill, 100px) 300px; }",
      ".foo{grid-template-columns:200px repeat(auto-fill,100px) 300px}",
    );
    minify_test(".foo { grid-template-columns: [linename1 linename2] 100px repeat(auto-fit, [linename1] 300px) [linename3]; }", ".foo{grid-template-columns:[linename1 linename2]100px repeat(auto-fit,[linename1]300px)[linename3]}");
    minify_test(
      ".foo { grid-template-rows: [linename1 linename2] 100px repeat(auto-fit, [linename1] 300px) [linename3]; }",
      ".foo{grid-template-rows:[linename1 linename2]100px repeat(auto-fit,[linename1]300px)[linename3]}",
    );

    minify_test(".foo { grid-auto-rows: auto; }", ".foo{grid-auto-rows:auto}");
    minify_test(".foo { grid-auto-rows: 1fr; }", ".foo{grid-auto-rows:1fr}");
    minify_test(".foo { grid-auto-rows: 100px; }", ".foo{grid-auto-rows:100px}");
    minify_test(
      ".foo { grid-auto-rows: min-content; }",
      ".foo{grid-auto-rows:min-content}",
    );
    minify_test(
      ".foo { grid-auto-rows: max-content; }",
      ".foo{grid-auto-rows:max-content}",
    );
    minify_test(
      ".foo { grid-auto-rows: minmax(100px,auto); }",
      ".foo{grid-auto-rows:minmax(100px,auto)}",
    );
    minify_test(
      ".foo { grid-auto-rows: fit-content(20%); }",
      ".foo{grid-auto-rows:fit-content(20%)}",
    );
    minify_test(
      ".foo { grid-auto-rows: 100px minmax(100px, auto) 10% 0.5fr fit-content(400px); }",
      ".foo{grid-auto-rows:100px minmax(100px,auto) 10% .5fr fit-content(400px)}",
    );
    minify_test(
      ".foo { grid-auto-columns: 100px minmax(100px, auto) 10% 0.5fr fit-content(400px); }",
      ".foo{grid-auto-columns:100px minmax(100px,auto) 10% .5fr fit-content(400px)}",
    );

    minify_test(
      r#"
      .foo {
        grid-template-areas: "head head"
                             "nav  main"
                             "foot ....";
      }
    "#,
      ".foo{grid-template-areas:\"head head\"\"nav main\"\"foot.\"}",
    );
    minify_test(
      r#"
      .foo {
        grid-template-areas: "head head"
                             "nav  main"
                             ".... foot";
      }
    "#,
      ".foo{grid-template-areas:\"head head\"\"nav main\"\".foot\"}",
    );
    minify_test(
      r#"
      .foo {
        grid-template-areas: "head head"
                             "nav  main"
                             ".... ....";
      }
    "#,
      ".foo{grid-template-areas:\"head head\"\"nav main\"\". .\"}",
    );

    test(
      r#"
      .foo {
        grid-template-areas: "head head" "nav  main" "foot ....";
      }
    "#,
      indoc! { r#"
      .foo {
        grid-template-areas: "head head"
                             "nav main"
                             "foot .";
      }
    "#},
    );

    minify_test(
      r#"
      .foo {
        grid-template: [header-top] "a   a   a"     [header-bottom]
                       [main-top] "b   b   b" 1fr [main-bottom];
      }
    "#,
      ".foo{grid-template:[header-top]\"a a a\"[header-bottom main-top]\"b b b\"1fr[main-bottom]}",
    );
    minify_test(
      r#"
      .foo {
        grid-template: "head head"
                       "nav  main" 1fr
                       "foot ....";
      }
    "#,
      ".foo{grid-template:\"head head\"\"nav main\"1fr\"foot.\"}",
    );
    minify_test(
      r#"
      .foo {
        grid-template: [header-top] "a   a   a"     [header-bottom]
                         [main-top] "b   b   b" 1fr [main-bottom]
                                  / auto 1fr auto;
      }
    "#,
      ".foo{grid-template:[header-top]\"a a a\"[header-bottom main-top]\"b b b\"1fr[main-bottom]/auto 1fr auto}",
    );

    minify_test(
      ".foo { grid-template: auto 1fr / auto 1fr auto; }",
      ".foo{grid-template:auto 1fr/auto 1fr auto}",
    );
    minify_test(
      ".foo { grid-template: [linename1 linename2] 100px repeat(auto-fit, [linename1] 300px) [linename3] / [linename1 linename2] 100px repeat(auto-fit, [linename1] 300px) [linename3]; }",
      ".foo{grid-template:[linename1 linename2]100px repeat(auto-fit,[linename1]300px)[linename3]/[linename1 linename2]100px repeat(auto-fit,[linename1]300px)[linename3]}"
    );

    test(
      ".foo{grid-template:[header-top]\"a a a\"[header-bottom main-top]\"b b b\"1fr[main-bottom]/auto 1fr auto}",
      indoc! {r#"
        .foo {
          grid-template: [header-top] "a a a" [header-bottom]
                         [main-top] "b b b" 1fr [main-bottom]
                         / auto 1fr auto;
        }
      "#},
    );
    test(
      ".foo{grid-template:[header-top]\"a a a\"[main-top]\"b b b\"1fr/auto 1fr auto}",
      indoc! {r#"
        .foo {
          grid-template: [header-top] "a a a"
                         [main-top] "b b b" 1fr
                         / auto 1fr auto;
        }
      "#},
    );

    minify_test(".foo { grid-auto-flow: row }", ".foo{grid-auto-flow:row}");
    minify_test(".foo { grid-auto-flow: column }", ".foo{grid-auto-flow:column}");
    minify_test(".foo { grid-auto-flow: row dense }", ".foo{grid-auto-flow:dense}");
    minify_test(".foo { grid-auto-flow: dense row }", ".foo{grid-auto-flow:dense}");
    minify_test(
      ".foo { grid-auto-flow: column dense }",
      ".foo{grid-auto-flow:column dense}",
    );
    minify_test(
      ".foo { grid-auto-flow: dense column }",
      ".foo{grid-auto-flow:column dense}",
    );

    minify_test(".foo { grid: none }", ".foo{grid:none}");
    minify_test(".foo { grid: \"a\" 100px \"b\" 1fr }", ".foo{grid:\"a\"100px\"b\"1fr}");
    minify_test(
      ".foo { grid: [linename1] \"a\" 100px [linename2] }",
      ".foo{grid:[linename1]\"a\"100px[linename2]}",
    );
    minify_test(
      ".foo { grid: \"a\" 200px \"b\" min-content }",
      ".foo{grid:\"a\"200px\"b\"min-content}",
    );
    minify_test(
      ".foo { grid: \"a\" minmax(100px, max-content) \"b\" 20% }",
      ".foo{grid:\"a\"minmax(100px,max-content)\"b\"20%}",
    );
    minify_test(".foo { grid: 100px / 200px }", ".foo{grid:100px/200px}");
    minify_test(
      ".foo { grid: minmax(400px, min-content) / repeat(auto-fill, 50px) }",
      ".foo{grid:minmax(400px,min-content)/repeat(auto-fill,50px)}",
    );

    minify_test(".foo { grid: 200px / auto-flow }", ".foo{grid:200px/auto-flow}");
    minify_test(".foo { grid: 30% / auto-flow dense }", ".foo{grid:30%/auto-flow dense}");
    minify_test(".foo { grid: 30% / dense auto-flow }", ".foo{grid:30%/auto-flow dense}");
    minify_test(
      ".foo { grid: repeat(3, [line1 line2 line3] 200px) / auto-flow 300px }",
      ".foo{grid:repeat(3,[line1 line2 line3]200px)/auto-flow 300px}",
    );
    minify_test(
      ".foo { grid: [line1] minmax(20em, max-content) / auto-flow dense 40% }",
      ".foo{grid:[line1]minmax(20em,max-content)/auto-flow dense 40%}",
    );
    minify_test(".foo { grid: none / auto-flow 1fr }", ".foo{grid:none/auto-flow 1fr}");

    minify_test(".foo { grid: auto-flow / 200px }", ".foo{grid:none/200px}");
    minify_test(".foo { grid: auto-flow dense / 30% }", ".foo{grid:auto-flow dense/30%}");
    minify_test(".foo { grid: dense auto-flow / 30% }", ".foo{grid:auto-flow dense/30%}");
    minify_test(
      ".foo { grid: auto-flow 300px / repeat(3, [line1 line2 line3] 200px) }",
      ".foo{grid:auto-flow 300px/repeat(3,[line1 line2 line3]200px)}",
    );
    minify_test(
      ".foo { grid: auto-flow dense 40% / [line1] minmax(20em, max-content) }",
      ".foo{grid:auto-flow dense 40%/[line1]minmax(20em,max-content)}",
    );

    minify_test(".foo { grid-row-start: auto }", ".foo{grid-row-start:auto}");
    minify_test(".foo { grid-row-start: some-area }", ".foo{grid-row-start:some-area}");
    minify_test(".foo { grid-row-start: 2 }", ".foo{grid-row-start:2}");
    minify_test(
      ".foo { grid-row-start: 2 some-line }",
      ".foo{grid-row-start:2 some-line}",
    );
    minify_test(
      ".foo { grid-row-start: some-line 2 }",
      ".foo{grid-row-start:2 some-line}",
    );
    minify_test(".foo { grid-row-start: span 3 }", ".foo{grid-row-start:span 3}");
    minify_test(
      ".foo { grid-row-start: span some-line }",
      ".foo{grid-row-start:span some-line}",
    );
    minify_test(
      ".foo { grid-row-start: span some-line 1 }",
      ".foo{grid-row-start:span some-line}",
    );
    minify_test(
      ".foo { grid-row-start: span 1 some-line }",
      ".foo{grid-row-start:span some-line}",
    );
    minify_test(
      ".foo { grid-row-start: span 5 some-line }",
      ".foo{grid-row-start:span 5 some-line}",
    );
    minify_test(
      ".foo { grid-row-start: span some-line 5 }",
      ".foo{grid-row-start:span 5 some-line}",
    );

    minify_test(
      ".foo { grid-row-end: span 1 some-line }",
      ".foo{grid-row-end:span some-line}",
    );
    minify_test(
      ".foo { grid-column-start: span 1 some-line }",
      ".foo{grid-column-start:span some-line}",
    );
    minify_test(
      ".foo { grid-column-end: span 1 some-line }",
      ".foo{grid-column-end:span some-line}",
    );

    minify_test(".foo { grid-row: 1 }", ".foo{grid-row:1}");
    minify_test(".foo { grid-row: 1 / auto }", ".foo{grid-row:1}");
    minify_test(".foo { grid-row: 1 / 1 }", ".foo{grid-row:1/1}");
    minify_test(".foo { grid-row: 1 / 3 }", ".foo{grid-row:1/3}");
    minify_test(".foo { grid-row: 1 / span 2 }", ".foo{grid-row:1/span 2}");
    minify_test(".foo { grid-row: main-start }", ".foo{grid-row:main-start}");
    minify_test(
      ".foo { grid-row: main-start / main-end }",
      ".foo{grid-row:main-start/main-end}",
    );
    minify_test(
      ".foo { grid-row: main-start / main-start }",
      ".foo{grid-row:main-start}",
    );
    minify_test(".foo { grid-column: 1 / auto }", ".foo{grid-column:1}");

    minify_test(".foo { grid-area: a }", ".foo{grid-area:a}");
    minify_test(".foo { grid-area: a / a / a / a }", ".foo{grid-area:a}");
    minify_test(".foo { grid-area: a / b / a / b }", ".foo{grid-area:a/b}");
    minify_test(".foo { grid-area: a / b / c / b }", ".foo{grid-area:a/b/c}");
    minify_test(".foo { grid-area: a / b / c / d }", ".foo{grid-area:a/b/c/d}");

    minify_test(".foo { grid-area: auto / auto / auto / auto }", ".foo{grid-area:auto}");
    minify_test(".foo { grid-area: 1 / auto }", ".foo{grid-area:1}");
    minify_test(".foo { grid-area: 1 / 2 / 3 / 4 }", ".foo{grid-area:1/2/3/4}");
    minify_test(".foo { grid-area: 1 / 1 / 1 / 1 }", ".foo{grid-area:1/1/1/1}");

    test(
      r#"
        .foo{
          grid-template-rows: auto 1fr;
          grid-template-columns: auto 1fr auto;
          grid-template-areas: none;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template: auto 1fr / auto 1fr auto;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-areas: "a a a"
                               "b b b";
          grid-template-rows: [header-top] auto [header-bottom main-top] 1fr [main-bottom];
          grid-template-columns: auto 1fr auto;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template: [header-top] "a a a" [header-bottom]
                         [main-top] "b b b" 1fr [main-bottom]
                         / auto 1fr auto;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-areas: "a a a"
                               "b b b";
          grid-template-columns: repeat(3, 1fr);
          grid-template-rows: auto 1fr;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template-rows: auto 1fr;
          grid-template-columns: repeat(3, 1fr);
          grid-template-areas: "a a a"
                               "b b b";
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-areas: "a a a"
                               "b b b";
          grid-template-columns: auto 1fr auto;
          grid-template-rows: repeat(2, 1fr);
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template-rows: repeat(2, 1fr);
          grid-template-columns: auto 1fr auto;
          grid-template-areas: "a a a"
                               "b b b";
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-areas: ". a a ."
                               ". b b .";
          grid-template-rows: auto 1fr;
          grid-template-columns: 10px 1fr 1fr 10px;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template: ". a a ."
                         ". b b ." 1fr
                         / 10px 1fr 1fr 10px;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-areas: none;
          grid-template-columns: auto 1fr auto;
          grid-template-rows: repeat(2, 1fr);
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template: repeat(2, 1fr) / auto 1fr auto;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-areas: none;
          grid-template-columns: none;
          grid-template-rows: none;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template: none;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-areas: "a a a"
                               "b b b";
          grid-template-rows: [header-top] auto [header-bottom main-top] 1fr [main-bottom];
          grid-template-columns: auto 1fr auto;
          grid-auto-flow: row;
          grid-auto-rows: auto;
          grid-auto-columns: auto;
        }
      "#,
      indoc! {r#"
        .foo {
          grid: [header-top] "a a a" [header-bottom]
                [main-top] "b b b" 1fr [main-bottom]
                / auto 1fr auto;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-areas: none;
          grid-template-columns: auto 1fr auto;
          grid-template-rows: repeat(2, 1fr);
          grid-auto-flow: row;
          grid-auto-rows: auto;
          grid-auto-columns: auto;
        }
      "#,
      indoc! {r#"
        .foo {
          grid: repeat(2, 1fr) / auto 1fr auto;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-areas: none;
          grid-template-columns: none;
          grid-template-rows: none;
          grid-auto-flow: row;
          grid-auto-rows: auto;
          grid-auto-columns: auto;
        }
      "#,
      indoc! {r#"
        .foo {
          grid: none;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-areas: "a a a"
                               "b b b";
          grid-template-rows: [header-top] auto [header-bottom main-top] 1fr [main-bottom];
          grid-template-columns: auto 1fr auto;
          grid-auto-flow: column;
          grid-auto-rows: 1fr;
          grid-auto-columns: 1fr;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template: [header-top] "a a a" [header-bottom]
                         [main-top] "b b b" 1fr [main-bottom]
                         / auto 1fr auto;
          grid-auto-rows: 1fr;
          grid-auto-columns: 1fr;
          grid-auto-flow: column;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-rows: auto 1fr;
          grid-template-columns: auto 1fr auto;
          grid-template-areas: none;
          grid-auto-flow: row;
          grid-auto-rows: auto;
          grid-auto-columns: auto;
        }
      "#,
      indoc! {r#"
        .foo {
          grid: auto 1fr / auto 1fr auto;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-rows: auto 1fr;
          grid-template-columns: auto 1fr auto;
          grid-template-areas: none;
          grid-auto-flow: column;
          grid-auto-rows: 1fr;
          grid-auto-columns: 1fr;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template: auto 1fr / auto 1fr auto;
          grid-auto-rows: 1fr;
          grid-auto-columns: 1fr;
          grid-auto-flow: column;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-rows: none;
          grid-template-columns: auto 1fr auto;
          grid-template-areas: none;
          grid-auto-flow: column;
          grid-auto-rows: 1fr;
          grid-auto-columns: 1fr;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template: none / auto 1fr auto;
          grid-auto-rows: 1fr;
          grid-auto-columns: 1fr;
          grid-auto-flow: column;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-rows: none;
          grid-template-columns: auto 1fr auto;
          grid-template-areas: none;
          grid-auto-flow: row;
          grid-auto-rows: 1fr;
          grid-auto-columns: auto;
        }
      "#,
      indoc! {r#"
        .foo {
          grid: auto-flow 1fr / auto 1fr auto;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-rows: none;
          grid-template-columns: auto 1fr auto;
          grid-template-areas: none;
          grid-auto-flow: row dense;
          grid-auto-rows: 1fr;
          grid-auto-columns: auto;
        }
      "#,
      indoc! {r#"
        .foo {
          grid: auto-flow dense 1fr / auto 1fr auto;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-rows: auto 1fr auto;
          grid-template-columns: none;
          grid-template-areas: none;
          grid-auto-flow: column;
          grid-auto-rows: auto;
          grid-auto-columns: 1fr;
        }
      "#,
      indoc! {r#"
        .foo {
          grid: auto 1fr auto / auto-flow 1fr;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-rows: auto 1fr auto;
          grid-template-columns: none;
          grid-template-areas: none;
          grid-auto-flow: column dense;
          grid-auto-rows: auto;
          grid-auto-columns: 1fr;
        }
      "#,
      indoc! {r#"
        .foo {
          grid: auto 1fr auto / auto-flow dense 1fr;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-template-rows: auto 1fr auto;
          grid-template-columns: none;
          grid-template-areas: none;
          grid-auto-flow: var(--auto-flow);
          grid-auto-rows: auto;
          grid-auto-columns: 1fr;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-template: auto 1fr auto / none;
          grid-auto-flow: var(--auto-flow);
          grid-auto-rows: auto;
          grid-auto-columns: 1fr;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid: auto 1fr auto / auto-flow dense 1fr;
          grid-template-rows: 1fr 1fr 1fr;
        }
      "#,
      indoc! {r#"
        .foo {
          grid: 1fr 1fr 1fr / auto-flow dense 1fr;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-row-start: a;
          grid-row-end: a;
          grid-column-start: a;
          grid-column-end: a;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-area: a;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-row-start: 1;
          grid-row-end: 2;
          grid-column-start: 3;
          grid-column-end: 4;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-area: 1 / 3 / 2 / 4;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-row-start: a;
          grid-row-end: a;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-row: a;
        }
      "#},
    );

    test(
      r#"
        .foo{
          grid-column-start: a;
          grid-column-end: a;
        }
      "#,
      indoc! {r#"
        .foo {
          grid-column: a;
        }
      "#},
    );
  }

  #[test]
  fn test_moz_document() {
    minify_test(
      r#"
      @-moz-document url-prefix() {
        h1 {
          color: yellow;
        }
      }
    "#,
      "@-moz-document url-prefix(){h1{color:#ff0}}",
    );
    minify_test(
      r#"
      @-moz-document url-prefix("") {
        h1 {
          color: yellow;
        }
      }
    "#,
      "@-moz-document url-prefix(){h1{color:#ff0}}",
    );
    error_test(
      "@-moz-document url-prefix(foo) {}",
      ParserError::UnexpectedToken(crate::properties::custom::Token::Ident("foo".into())),
    );
    error_test(
      "@-moz-document url-prefix(\"foo\") {}",
      ParserError::UnexpectedToken(crate::properties::custom::Token::String("foo".into())),
    );
  }

  #[test]
  fn test_custom_properties() {
    minify_test(".foo { --test: ; }", ".foo{--test: }");
    minify_test(".foo { --test:  ; }", ".foo{--test: }");
    minify_test(".foo { --test: foo; }", ".foo{--test:foo}");
    minify_test(".foo { --test:  foo; }", ".foo{--test:foo}");
    minify_test(".foo { --test: foo ; }", ".foo{--test:foo}");
    minify_test(".foo { --test: foo  ; }", ".foo{--test:foo}");
    minify_test(".foo { --test:foo; }", ".foo{--test:foo}");
    minify_test(".foo { --test:foo ; }", ".foo{--test:foo}");
    minify_test(".foo { --test: var(--foo, 20px); }", ".foo{--test:var(--foo,20px)}");
    minify_test(
      ".foo { transition: var(--foo, 20px),\nvar(--bar, 40px); }",
      ".foo{transition:var(--foo,20px),var(--bar,40px)}",
    );
    minify_test(
      ".foo { background: var(--color) var(--image); }",
      ".foo{background:var(--color)var(--image)}",
    );
    minify_test(
      ".foo { height: calc(var(--spectrum-global-dimension-size-300) / 2);",
      ".foo{height:calc(var(--spectrum-global-dimension-size-300)/2)}",
    );
    minify_test(
      ".foo { color: var(--color, rgb(255, 255, 0)); }",
      ".foo{color:var(--color,#ff0)}",
    );
    minify_test(
      ".foo { color: var(--color, #ffff00); }",
      ".foo{color:var(--color,#ff0)}",
    );
    minify_test(
      ".foo { color: var(--color, rgb(var(--red), var(--green), 0)); }",
      ".foo{color:var(--color,rgb(var(--red),var(--green),0))}",
    );
    minify_test(".foo { --test: .5s; }", ".foo{--test:.5s}");
    minify_test(".foo { --theme-sizes-1\\/12: 2 }", ".foo{--theme-sizes-1\\/12:2}");
    minify_test(".foo { --test: 0px; }", ".foo{--test:0px}");

    prefix_test(
      r#"
      .foo {
        --custom: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        --custom: #b32323;
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          --custom: lab(40% 56.6 39);
        }
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --custom: lab(40% 56.6 39) !important;
      }
    "#,
      indoc! {r#"
      .foo {
        --custom: #b32323 !important;
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          --custom: lab(40% 56.6 39) !important;
        }
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --custom: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        --custom: #b32323;
      }

      @supports (color: color(display-p3 0 0 0)) {
        .foo {
          --custom: color(display-p3 .643308 .192455 .167712);
        }
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          --custom: lab(40% 56.6 39);
        }
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --custom: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        --custom: color(display-p3 .643308 .192455 .167712);
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          --custom: lab(40% 56.6 39);
        }
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --custom: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        --custom: lab(40% 56.6 39);
      }
    "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --custom: oklab(59.686% 0.1009 0.1192);
      }
    "#,
      indoc! {r#"
      .foo {
        --custom: lab(52.2319% 40.1449 59.9171);
      }
    "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --custom: oklab(59.686% 0.1009 0.1192);
      }
    "#,
      indoc! {r#"
      .foo {
        --custom: color(display-p3 .724144 .386777 .148795);
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          --custom: lab(52.2319% 40.1449 59.9171);
        }
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --custom: oklab(59.686% 0.1009 0.1192);
      }
    "#,
      indoc! {r#"
      .foo {
        --custom: #c65d07;
      }

      @supports (color: color(display-p3 0 0 0)) {
        .foo {
          --custom: color(display-p3 .724144 .386777 .148795);
        }
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          --custom: lab(52.2319% 40.1449 59.9171);
        }
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --foo: oklab(59.686% 0.1009 0.1192);
        --bar: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        --foo: #c65d07;
        --bar: #b32323;
      }

      @supports (color: color(display-p3 0 0 0)) {
        .foo {
          --foo: color(display-p3 .724144 .386777 .148795);
          --bar: color(display-p3 .643308 .192455 .167712);
        }
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          --foo: lab(52.2319% 40.1449 59.9171);
          --bar: lab(40% 56.6 39);
        }
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --foo: color(display-p3 0 1 0);
      }
    "#,
      indoc! {r#"
      .foo {
        --foo: #00f942;
      }

      @supports (color: color(display-p3 0 0 0)) {
        .foo {
          --foo: color(display-p3 0 1 0);
        }
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --foo: color(display-p3 0 1 0);
      }
    "#,
      indoc! {r#"
      .foo {
        --foo: color(display-p3 0 1 0);
      }
    "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --foo: color(display-p3 0 1 0);
      }
    "#,
      indoc! {r#"
      .foo {
        --foo: #00f942;
      }

      @supports (color: color(display-p3 0 0 0)) {
        .foo {
          --foo: color(display-p3 0 1 0);
        }
      }
    "#},
      Browsers {
        safari: Some(15 << 16),
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --foo: color(display-p3 0 1 0);
      }
    "#,
      indoc! {r#"
      .foo {
        --foo: #00f942;
      }

      @supports (color: color(display-p3 0 0 0)) {
        .foo {
          --foo: color(display-p3 0 1 0);
        }
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        text-decoration: underline;
      }

      .foo {
        --custom: lab(40% 56.6 39);
      }
    "#,
      indoc! {r#"
      .foo {
        --custom: #b32323;
        text-decoration: underline;
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          --custom: lab(40% 56.6 39);
        }
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      .foo {
        --custom: lab(40% 56.6 39);
      }

      .foo {
        text-decoration: underline;
      }
    "#,
      indoc! {r#"
      .foo {
        --custom: #b32323;
      }

      @supports (color: lab(0% 0 0)) {
        .foo {
          --custom: lab(40% 56.6 39);
        }
      }

      .foo {
        text-decoration: underline;
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      @keyframes foo {
        from {
          --custom: lab(40% 56.6 39);
        }

        to {
          --custom: lch(50.998% 135.363 338);
        }
      }
    "#,
      indoc! {r#"
      @keyframes foo {
        from {
          --custom: #b32323;
        }

        to {
          --custom: #ee00be;
        }
      }

      @supports (color: lab(0% 0 0)) {
        @keyframes foo {
          from {
            --custom: lab(40% 56.6 39);
          }

          to {
            --custom: lab(50.998% 125.506 -50.7078);
          }
        }
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      @keyframes foo {
        from {
          --custom: lab(40% 56.6 39);
        }

        to {
          --custom: lch(50.998% 135.363 338);
        }
      }
    "#,
      indoc! {r#"
      @keyframes foo {
        from {
          --custom: #b32323;
        }

        to {
          --custom: #ee00be;
        }
      }

      @supports (color: color(display-p3 0 0 0)) {
        @keyframes foo {
          from {
            --custom: color(display-p3 .643308 .192455 .167712);
          }

          to {
            --custom: color(display-p3 .972962 -.362078 .804206);
          }
        }
      }

      @supports (color: lab(0% 0 0)) {
        @keyframes foo {
          from {
            --custom: lab(40% 56.6 39);
          }

          to {
            --custom: lab(50.998% 125.506 -50.7078);
          }
        }
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      @keyframes foo {
        from {
          --custom: #ff0;
          opacity: 0;
        }

        to {
          --custom: lch(50.998% 135.363 338);
          opacity: 1;
        }
      }
    "#,
      indoc! {r#"
      @keyframes foo {
        from {
          --custom: #ff0;
          opacity: 0;
        }

        to {
          --custom: #ee00be;
          opacity: 1;
        }
      }

      @supports (color: lab(0% 0 0)) {
        @keyframes foo {
          from {
            --custom: #ff0;
            opacity: 0;
          }

          to {
            --custom: lab(50.998% 125.506 -50.7078);
            opacity: 1;
          }
        }
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      @keyframes foo {
        from {
          text-decoration: var(--foo) lab(29.2345% 39.3825 20.0664);
        }
      }
    "#,
      indoc! {r#"
      @keyframes foo {
        from {
          text-decoration: var(--foo) #7d2329;
        }
      }

      @supports (color: lab(0% 0 0)) {
        @keyframes foo {
          from {
            text-decoration: var(--foo) lab(29.2345% 39.3825 20.0664);
          }
        }
      }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_charset() {
    test(
      r#"
      @charset "UTF-8";

      .foo {
        color: red;
      }

      @charset "UTF-8";

      .bar {
        color: yellow;
      }
    "#,
      indoc! { r#"
      .foo {
        color: red;
      }

      .bar {
        color: #ff0;
      }
    "#},
    )
  }

  #[test]
  fn test_style_attr() {
    attr_test("color: yellow; flex: 1 1 auto", "color: #ff0; flex: auto", false, None);
    attr_test("color: yellow; flex: 1 1 auto", "color:#ff0;flex:auto", true, None);
    attr_test(
      "border-inline-start: 2px solid red",
      "border-inline-start: 2px solid red",
      false,
      Some(Browsers {
        safari: Some(12 << 16),
        ..Browsers::default()
      }),
    );
    attr_test(
      "color: lab(40% 56.6 39);",
      "color:#b32323;color:lab(40% 56.6 39)",
      true,
      Some(Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      }),
    );
    attr_test(
      "--foo: lab(40% 56.6 39);",
      "--foo:#b32323",
      true,
      Some(Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      }),
    );
    attr_test(
      "text-decoration: var(--foo) lab(40% 56.6 39);",
      "text-decoration:var(--foo)#b32323",
      true,
      Some(Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      }),
    );
  }

  #[test]
  fn test_nesting() {
    nesting_test(
      r#"
        .foo {
          color: blue;
          & > .bar { color: red; }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;
        }

        .foo > .bar {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: blue;
          &.bar { color: red; }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;
        }

        .foo.bar {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo, .bar {
          color: blue;
          & + .baz, &.qux { color: red; }
        }
      "#,
      indoc! {r#"
        .foo, .bar {
          color: #00f;
        }

        :is(.foo, .bar) + .baz, :is(.foo, .bar).qux {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: blue;
          & .bar & .baz & .qux { color: red; }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;
        }

        .foo .bar .foo .baz .foo .qux {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: blue;
          & { padding: 2ch; }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;
        }

        .foo {
          padding: 2ch;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: blue;
          && { padding: 2ch; }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;
        }

        .foo.foo {
          padding: 2ch;
        }
      "#},
    );

    nesting_test(
      r#"
        .error, .invalid {
          &:hover > .baz { color: red; }
        }
      "#,
      indoc! {r#"
        :is(.error, .invalid):hover > .baz {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          &:is(.bar, &.baz) { color: red; }
        }
      "#,
      indoc! {r#"
        .foo:is(.bar, .foo.baz) {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        figure {
          margin: 0;

          & > figcaption {
            background: hsl(0 0% 0% / 50%);

            & > p {
              font-size: .9rem;
            }
          }
        }
      "#,
      indoc! {r#"
        figure {
          margin: 0;
        }

        figure > figcaption {
          background: #00000080;
        }

        figure > figcaption > p {
          font-size: .9rem;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          display: grid;

          @media (orientation: landscape) {
            grid-auto-flow: column;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          display: grid;
        }

        @media (orientation: landscape) {
          .foo {
            grid-auto-flow: column;
          }
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          display: grid;

          @media (orientation: landscape) {
            grid-auto-flow: column;

            @media (width > 1024px) {
              max-inline-size: 1024px;
            }
          }
        }
      "#,
      indoc! {r#"
        .foo {
          display: grid;
        }

        @media (orientation: landscape) {
          .foo {
            grid-auto-flow: column;
          }

          @media not (max-width: 1024px) {
            .foo {
              max-inline-size: 1024px;
            }
          }
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          @media (min-width: 640px) {
            color: red !important;
          }
        }
      "#,
      indoc! {r#"
        @media (min-width: 640px) {
          .foo {
            color: red !important;
          }
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          display: grid;

          @supports (foo: bar) {
            grid-auto-flow: column;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          display: grid;
        }

        @supports (foo: bar) {
          .foo {
            grid-auto-flow: column;
          }
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          display: grid;

          @container (min-width: 100px) {
            grid-auto-flow: column;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          display: grid;
        }

        @container (width >= 100px) {
          .foo {
            grid-auto-flow: column;
          }
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          display: grid;

          @layer test {
            grid-auto-flow: column;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          display: grid;
        }

        @layer test {
          .foo {
            grid-auto-flow: column;
          }
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          display: grid;

          @layer {
            grid-auto-flow: column;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          display: grid;
        }

        @layer {
          .foo {
            grid-auto-flow: column;
          }
        }
      "#},
    );

    nesting_test(
      r#"
        @namespace "http://example.com/foo";
        @namespace toto "http://toto.example.org";

        .foo {
          &div {
            color: red;
          }

          &* {
            color: green;
          }

          &|x {
            color: red;
          }

          &*|x {
            color: green;
          }

          &toto|x {
            color: red;
          }
        }
      "#,
      indoc! {r#"
        @namespace "http://example.com/foo";
        @namespace toto "http://toto.example.org";

        div.foo {
          color: red;
        }

        *.foo {
          color: green;
        }

        |x.foo {
          color: red;
        }

        *|x.foo {
          color: green;
        }

        toto|x.foo {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          &article > figure {
            color: red;
          }
        }
      "#,
      indoc! {r#"
        article.foo > figure {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        div {
          &.bar {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        div.bar {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        div > .foo {
          &span {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        span:is(div > .foo) {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          & h1 {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        .foo h1 {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          &h1 {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        h1:is(.foo .bar) {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo.bar {
          &h1 {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        h1.foo.bar {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          &h1 .baz {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        h1:is(.foo .bar) .baz {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          &.baz {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        .foo .bar.baz {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: red;
          @nest .parent & {
            color: blue;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          color: red;
        }

        .parent .foo {
          color: #00f;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: red;
          @nest :not(&) {
            color: blue;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          color: red;
        }

        :not(.foo) {
          color: #00f;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: blue;
          @nest .bar & {
            color: red;
            &.baz {
              color: green;
            }
          }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;
        }

        .bar .foo {
          color: red;
        }

        .bar .foo.baz {
          color: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          @nest :not(&) {
            color: red;
          }

          & h1 {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        :not(.foo) {
          color: red;
        }

        .foo h1 {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          & h1 {
            background: green;
          }

          @nest :not(&) {
            color: red;
          }
        }
      "#,
      indoc! {r#"
        .foo h1 {
          background: green;
        }

        :not(.foo) {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          @nest h1& {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        h1:is(.foo .bar) {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        @namespace "http://example.com/foo";
        @namespace toto "http://toto.example.org";

        div {
          @nest .foo& {
            color: red;
          }
        }

        * {
          @nest .foo& {
            color: red;
          }
        }

        |x {
          @nest .foo& {
            color: red;
          }
        }

        *|x {
          @nest .foo& {
            color: red;
          }
        }

        toto|x {
          @nest .foo& {
            color: red;
          }
        }
      "#,
      indoc! {r#"
        @namespace "http://example.com/foo";
        @namespace toto "http://toto.example.org";

        .foo:is(div) {
          color: red;
        }

        .foo:is(*) {
          color: red;
        }

        .foo:is(|x) {
          color: red;
        }

        .foo:is(*|x) {
          color: red;
        }

        .foo:is(toto|x) {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          @nest h1 .baz& {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        h1 .baz:is(.foo .bar) {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          @nest .baz& {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        .baz:is(.foo .bar) {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          @nest .baz & {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        .baz :is(.foo .bar) {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: red;
          @nest & > .bar {
            color: blue;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          color: red;
        }

        .foo > .bar {
          color: #00f;
        }
      "#},
    );

    nesting_test(
      r#"
      .foo {
        color: red;
        .bar {
          color: blue;
        }
      }
      "#,
      indoc! {r#"
      .foo {
        color: red;
      }

      .foo .bar {
        color: #00f;
      }
      "#},
    );

    nesting_test(
      r#"
      .foo {
        color: red;
        .bar & {
          color: blue;
        }
      }
      "#,
      indoc! {r#"
      .foo {
        color: red;
      }

      .bar .foo {
        color: #00f;
      }
      "#},
    );

    nesting_test(
      r#"
      .foo {
        color: red;
        + .bar + & { color: blue; }
      }
      "#,
      indoc! {r#"
      .foo {
        color: red;
      }

      .foo + .bar + .foo {
        color: #00f;
      }
      "#},
    );

    nesting_test(
      r#"
      .foo {
        color: red;
        .bar & {
          color: blue;
        }
      }
      "#,
      indoc! {r#"
      .foo {
        color: red;
      }

      .bar .foo {
        color: #00f;
      }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: red;
          .parent & {
            color: blue;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          color: red;
        }

        .parent .foo {
          color: #00f;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: red;
          :not(&) {
            color: blue;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          color: red;
        }

        :not(.foo) {
          color: #00f;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: blue;
          .bar & {
            color: red;
            &.baz {
              color: green;
            }
          }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;
        }

        .bar .foo {
          color: red;
        }

        .bar .foo.baz {
          color: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          :not(&) {
            color: red;
          }

          & h1 {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        :not(.foo) {
          color: red;
        }

        .foo h1 {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          & h1 {
            background: green;
          }

          :not(&) {
            color: red;
          }
        }
      "#,
      indoc! {r#"
        .foo h1 {
          background: green;
        }

        :not(.foo) {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          :is(h1)& {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        :is(h1):is(.foo .bar) {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        @namespace "http://example.com/foo";
        @namespace toto "http://toto.example.org";

        div {
          .foo& {
            color: red;
          }
        }

        * {
          .foo& {
            color: red;
          }
        }

        |x {
          .foo& {
            color: red;
          }
        }

        *|x {
          .foo& {
            color: red;
          }
        }

        toto|x {
          .foo& {
            color: red;
          }
        }
      "#,
      indoc! {r#"
        @namespace "http://example.com/foo";
        @namespace toto "http://toto.example.org";

        .foo:is(div) {
          color: red;
        }

        .foo:is(*) {
          color: red;
        }

        .foo:is(|x) {
          color: red;
        }

        .foo:is(*|x) {
          color: red;
        }

        .foo:is(toto|x) {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          :is(h1) .baz& {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        :is(h1) .baz:is(.foo .bar) {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          .baz& {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        .baz:is(.foo .bar) {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo .bar {
          .baz & {
            background: green;
          }
        }
      "#,
      indoc! {r#"
        .baz :is(.foo .bar) {
          background: green;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          .bar {
            color: blue;
          }
          color: red;
        }
      "#,
      indoc! {r#"
        .foo {
          color: red;
        }

        .foo .bar {
          color: #00f;
        }
      "#},
    );

    nesting_test(
      r#"
        article {
          color: green;
          & { color: blue; }
          color: red;
        }
      "#,
      indoc! {r#"
        article {
          color: red;
        }

        article {
          color: #00f;
        }
      "#},
    );

    nesting_test(
      r#"
        & .foo {
          color: red;
        }
      "#,
      indoc! {r#"
        :scope .foo {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        &.foo {
          color: red;
        }
      "#,
      indoc! {r#"
        :scope.foo {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo& {
          color: red;
        }
      "#,
      indoc! {r#"
        .foo:scope {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        &html {
          color: red;
        }
      "#,
      indoc! {r#"
        html:scope {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        .foo {
          color: blue;
          div {
            color: red;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;
        }

        .foo div {
          color: red;
        }
      "#},
    );

    nesting_test(
      r#"
        div {
          color: blue;

          button:focus {
            color: red;
          }
        }
      "#,
      indoc! {r#"
        div {
          color: #00f;
        }

        div button:focus {
          color: red;
        }
      "#},
    );
    nesting_test(
      r#"
        div {
          color: blue;

          --button:focus {
            color: red;
          }
        }
      "#,
      indoc! {r#"
        div {
          color: #00f;
          --button: focus { color: red; };
        }
      "#},
    );

    nesting_test_no_targets(
      r#"
        .foo {
          color: blue;
          @nest .bar & {
            color: red;
            &.baz {
              color: green;
            }
          }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;

          @nest .bar & {
            color: red;

            &.baz {
              color: green;
            }
          }
        }
      "#},
    );

    nesting_test_no_targets(
      r#"
        .foo {
          color: blue;
          &div {
            color: red;
          }

          &span {
            color: purple;
          }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;

          &div {
            color: red;
          }

          &span {
            color: purple;
          }
        }
      "#},
    );

    nesting_test_no_targets(
      r#"
        .error, .invalid {
          &:hover > .baz { color: red; }
        }
      "#,
      indoc! {r#"
        .error, .invalid {
          &:hover > .baz {
            color: red;
          }
        }
      "#},
    );

    nesting_test_with_targets(
      r#"
        .foo {
          color: blue;
          & > .bar { color: red; }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;
        }

        .foo > .bar {
          color: red;
        }
      "#},
      Targets {
        browsers: Some(Browsers {
          chrome: Some(112 << 16),
          ..Browsers::default()
        }),
        include: Features::Nesting,
        exclude: Features::empty(),
      },
    );
    nesting_test_with_targets(
      r#"
        .foo {
          color: blue;
          & > .bar { color: red; }
        }
      "#,
      indoc! {r#"
        .foo {
          color: #00f;

          & > .bar {
            color: red;
          }
        }
      "#},
      Targets {
        browsers: Some(Browsers {
          chrome: Some(50 << 16),
          ..Browsers::default()
        }),
        include: Features::empty(),
        exclude: Features::Nesting,
      },
    );

    let mut stylesheet = StyleSheet::parse(
      r#"
      .foo {
        color: blue;
        .bar {
          color: red;
        }
      }
      "#,
      ParserOptions::default(),
    )
    .unwrap();
    stylesheet.minify(MinifyOptions::default()).unwrap();
    let res = stylesheet
      .to_css(PrinterOptions {
        minify: true,
        ..PrinterOptions::default()
      })
      .unwrap();
    assert_eq!(res.code, ".foo{color:#00f;& .bar{color:red}}");

    nesting_test_with_targets(
      r#"
        .a {
          &.b,
          &.c {
            &.d {
              color: red;
            }
          }
        }
      "#,
      indoc! {r#"
        .a.b.d {
          color: red;
        }

        .a.c.d {
          color: red;
        }
      "#},
      Targets {
        browsers: Some(Browsers {
          safari: Some(13 << 16),
          ..Browsers::default()
        }),
        include: Features::Nesting,
        exclude: Features::empty(),
      },
    );
  }

  #[test]
  fn test_css_modules() {
    css_modules_test(
      r#"
      .foo {
        color: red;
      }

      #id {
        animation: 2s test;
      }

      @keyframes test {
        from { color: red }
        to { color: yellow }
      }

      @counter-style circles {
        symbols: Ⓐ Ⓑ Ⓒ;
      }

      ul {
        list-style: circles;
      }

      ol {
        list-style-type: none;
      }

      li {
        list-style-type: disc;
      }

      @keyframes fade {
        from { opacity: 0 }
        to { opacity: 1 }
      }
    "#,
      indoc! {r#"
      .EgL3uq_foo {
        color: red;
      }

      #EgL3uq_id {
        animation: 2s EgL3uq_test;
      }

      @keyframes EgL3uq_test {
        from {
          color: red;
        }

        to {
          color: #ff0;
        }
      }

      @counter-style EgL3uq_circles {
        symbols: Ⓐ Ⓑ Ⓒ;
      }

      ul {
        list-style: EgL3uq_circles;
      }

      ol {
        list-style-type: none;
      }

      li {
        list-style-type: disc;
      }

      @keyframes EgL3uq_fade {
        from {
          opacity: 0;
        }

        to {
          opacity: 1;
        }
      }
    "#},
      map! {
        "foo" => "EgL3uq_foo",
        "id" => "EgL3uq_id",
        "test" => "EgL3uq_test" referenced: true,
        "circles" => "EgL3uq_circles" referenced: true,
        "fade" => "EgL3uq_fade"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      .foo {
        color: red;
      }

      #id {
        animation: 2s test;
      }

      @keyframes test {
        from { color: red }
        to { color: yellow }
      }
    "#,
      indoc! {r#"
      .EgL3uq_foo {
        color: red;
      }

      #EgL3uq_id {
        animation: 2s test;
      }

      @keyframes test {
        from {
          color: red;
        }

        to {
          color: #ff0;
        }
      }
    "#},
      map! {
        "foo" => "EgL3uq_foo",
        "id" => "EgL3uq_id"
      },
      HashMap::new(),
      crate::css_modules::Config {
        animation: false,
        // custom_idents: false,
        ..Default::default()
      },
      false,
    );

    css_modules_test(
      r#"
      @counter-style circles {
        symbols: Ⓐ Ⓑ Ⓒ;
      }

      ul {
        list-style: circles;
      }

      ol {
        list-style-type: none;
      }

      li {
        list-style-type: disc;
      }
    "#,
      indoc! {r#"
      @counter-style circles {
        symbols: Ⓐ Ⓑ Ⓒ;
      }

      ul {
        list-style: circles;
      }

      ol {
        list-style-type: none;
      }

      li {
        list-style-type: disc;
      }
    "#},
      map! {
        "circles" => "EgL3uq_circles" referenced: true
      },
      HashMap::new(),
      crate::css_modules::Config {
        custom_idents: false,
        ..Default::default()
      },
      false,
    );

    #[cfg(feature = "grid")]
    css_modules_test(
      r#"
      body {
        grid: [header-top] "a a a" [header-bottom]
              [main-top] "b b b" 1fr [main-bottom]
              / auto 1fr auto;
      }

      header {
        grid-area: a;
      }

      main {
        grid-row: main-top / main-bottom;
      }
    "#,
      indoc! {r#"
      body {
        grid: [EgL3uq_header-top] "EgL3uq_a EgL3uq_a EgL3uq_a" [EgL3uq_header-bottom]
              [EgL3uq_main-top] "EgL3uq_b EgL3uq_b EgL3uq_b" 1fr [EgL3uq_main-bottom]
              / auto 1fr auto;
      }

      header {
        grid-area: EgL3uq_a;
      }

      main {
        grid-row: EgL3uq_main-top / EgL3uq_main-bottom;
      }
    "#},
      map! {
        "header-top" => "EgL3uq_header-top",
        "header-bottom" => "EgL3uq_header-bottom",
        "main-top" => "EgL3uq_main-top",
        "main-bottom" => "EgL3uq_main-bottom",
        "a" => "EgL3uq_a",
        "b" => "EgL3uq_b"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    #[cfg(feature = "grid")]
    css_modules_test(
      r#"
        .grid {
          grid-template-areas: "foo";
        }

        .foo {
          grid-area: foo;
        }

        .bar {
          grid-column-start: foo-start;
        }
      "#,
      indoc! {r#"
        .EgL3uq_grid {
          grid-template-areas: "EgL3uq_foo";
        }

        .EgL3uq_foo {
          grid-area: EgL3uq_foo;
        }

        .EgL3uq_bar {
          grid-column-start: EgL3uq_foo-start;
        }
      "#},
      map! {
        "foo" => "EgL3uq_foo",
        "foo-start" => "EgL3uq_foo-start",
        "grid" => "EgL3uq_grid",
        "bar" => "EgL3uq_bar"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    #[cfg(feature = "grid")]
    css_modules_test(
      r#"
        .grid {
          grid-template-areas: "foo";
        }

        .foo {
          grid-area: foo;
        }

        .bar {
          grid-column-start: foo-start;
        }
      "#,
      indoc! {r#"
        .EgL3uq_grid {
          grid-template-areas: "foo";
        }

        .EgL3uq_foo {
          grid-area: foo;
        }

        .EgL3uq_bar {
          grid-column-start: foo-start;
        }
      "#},
      map! {
        "foo" => "EgL3uq_foo",
        "grid" => "EgL3uq_grid",
        "bar" => "EgL3uq_bar"
      },
      HashMap::new(),
      crate::css_modules::Config {
        grid: false,
        ..Default::default()
      },
      false,
    );

    css_modules_test(
      r#"
      test {
        transition-property: opacity;
      }
    "#,
      indoc! {r#"
      test {
        transition-property: opacity;
      }
    "#},
      map! {},
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      :global(.foo) {
        color: red;
      }

      :local(.bar) {
        color: yellow;
      }

      .bar :global(.baz) {
        color: purple;
      }
    "#,
      indoc! {r#"
      .foo {
        color: red;
      }

      .EgL3uq_bar {
        color: #ff0;
      }

      .EgL3uq_bar .baz {
        color: purple;
      }
    "#},
      map! {
        "bar" => "EgL3uq_bar"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    // :global(:local(.hi)) {
    //   color: green;
    // }

    css_modules_test(
      r#"
      .test {
        composes: foo;
        background: white;
      }

      .foo {
        color: red;
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        background: #fff;
      }

      .EgL3uq_foo {
        color: red;
      }
    "#},
      map! {
        "test" => "EgL3uq_test" "EgL3uq_foo",
        "foo" => "EgL3uq_foo"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      .a, .b {
        composes: foo;
        background: white;
      }

      .foo {
        color: red;
      }
    "#,
      indoc! {r#"
      .EgL3uq_a, .EgL3uq_b {
        background: #fff;
      }

      .EgL3uq_foo {
        color: red;
      }
    "#},
      map! {
        "a" => "EgL3uq_a" "EgL3uq_foo",
        "b" => "EgL3uq_b" "EgL3uq_foo",
        "foo" => "EgL3uq_foo"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      .test {
        composes: foo bar;
        background: white;
      }

      .foo {
        color: red;
      }

      .bar {
        color: yellow;
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        background: #fff;
      }

      .EgL3uq_foo {
        color: red;
      }

      .EgL3uq_bar {
        color: #ff0;
      }
    "#},
      map! {
        "test" => "EgL3uq_test" "EgL3uq_foo" "EgL3uq_bar",
        "foo" => "EgL3uq_foo",
        "bar" => "EgL3uq_bar"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      .test {
        composes: foo from global;
        background: white;
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        background: #fff;
      }
    "#},
      map! {
        "test" => "EgL3uq_test" "foo" global: true
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      .test {
        composes: foo bar from global;
        background: white;
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        background: #fff;
      }
    "#},
      map! {
        "test" => "EgL3uq_test" "foo" global: true "bar" global: true
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      .test {
        composes: foo from "foo.css";
        background: white;
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        background: #fff;
      }
    "#},
      map! {
        "test" => "EgL3uq_test" "foo" from "foo.css"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      .test {
        composes: foo bar from "foo.css";
        background: white;
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        background: #fff;
      }
    "#},
      map! {
        "test" => "EgL3uq_test" "foo" from "foo.css" "bar" from "foo.css"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      .test {
        composes: foo;
        composes: foo from "foo.css";
        composes: bar from "bar.css";
        background: white;
      }

      .foo {
        color: red;
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        background: #fff;
      }

      .EgL3uq_foo {
        color: red;
      }
    "#},
      map! {
        "test" => "EgL3uq_test" "EgL3uq_foo" "foo" from "foo.css" "bar" from "bar.css",
        "foo" => "EgL3uq_foo"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      .foo {
        color: red;
      }
    "#,
      indoc! {r#"
      .test-EgL3uq-foo {
        color: red;
      }
    "#},
      map! {
        "foo" => "test-EgL3uq-foo"
      },
      HashMap::new(),
      crate::css_modules::Config {
        pattern: crate::css_modules::Pattern::parse("test-[hash]-[local]").unwrap(),
        ..Default::default()
      },
      false,
    );

    let stylesheet = StyleSheet::parse(
      r#"
        .grid {
          grid-template-areas: "foo";
        }

        .foo {
          grid-area: foo;
        }

        .bar {
          grid-column-start: foo-start;
        }
      "#,
      ParserOptions {
        css_modules: Some(crate::css_modules::Config {
          pattern: crate::css_modules::Pattern::parse("test-[local]-[hash]").unwrap(),
          ..Default::default()
        }),
        ..ParserOptions::default()
      },
    )
    .unwrap();
    if let Err(err) = stylesheet.to_css(PrinterOptions::default()) {
      assert_eq!(err.kind, PrinterErrorKind::InvalidCssModulesPatternInGrid);
    } else {
      unreachable!()
    }

    css_modules_test(
      r#"
      @property --foo {
        syntax: '<color>';
        inherits: false;
        initial-value: yellow;
      }

      .foo {
        --foo: red;
        color: var(--foo);
      }
    "#,
      indoc! {r#"
      @property --foo {
        syntax: "<color>";
        inherits: false;
        initial-value: #ff0;
      }

      .EgL3uq_foo {
        --foo: red;
        color: var(--foo);
      }
    "#},
      map! {
        "foo" => "EgL3uq_foo"
      },
      HashMap::new(),
      Default::default(),
      false,
    );

    css_modules_test(
      r#"
      @property --foo {
        syntax: '<color>';
        inherits: false;
        initial-value: yellow;
      }

      @font-palette-values --Cooler {
        font-family: Bixa;
        base-palette: 1;
        override-colors: 1 #7EB7E4;
      }

      .foo {
        --foo: red;
        --bar: green;
        color: var(--foo);
        font-palette: --Cooler;
      }

      .bar {
        color: var(--color from "./b.css");
      }
    "#,
      indoc! {r#"
      @property --EgL3uq_foo {
        syntax: "<color>";
        inherits: false;
        initial-value: #ff0;
      }

      @font-palette-values --EgL3uq_Cooler {
        font-family: Bixa;
        base-palette: 1;
        override-colors: 1 #7eb7e4;
      }

      .EgL3uq_foo {
        --EgL3uq_foo: red;
        --EgL3uq_bar: green;
        color: var(--EgL3uq_foo);
        font-palette: --EgL3uq_Cooler;
      }

      .EgL3uq_bar {
        color: var(--ma1CsG);
      }
    "#},
      map! {
        "foo" => "EgL3uq_foo",
        "--foo" => "--EgL3uq_foo" referenced: true,
        "--bar" => "--EgL3uq_bar",
        "bar" => "EgL3uq_bar",
        "--Cooler" => "--EgL3uq_Cooler" referenced: true
      },
      HashMap::from([(
        "--ma1CsG".into(),
        CssModuleReference::Dependency {
          name: "--color".into(),
          specifier: "./b.css".into(),
        },
      )]),
      crate::css_modules::Config {
        dashed_idents: true,
        ..Default::default()
      },
      false,
    );

    css_modules_test(
      r#"
      .test {
        animation: rotate var(--duration) linear infinite;
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        animation: EgL3uq_rotate var(--duration) linear infinite;
      }
    "#},
      map! {
        "test" => "EgL3uq_test",
        "rotate" => "EgL3uq_rotate" referenced: true
      },
      HashMap::new(),
      Default::default(),
      false,
    );
    css_modules_test(
      r#"
      .test {
        animation: none var(--duration);
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        animation: none var(--duration);
      }
    "#},
      map! {
        "test" => "EgL3uq_test"
      },
      HashMap::new(),
      Default::default(),
      false,
    );
    css_modules_test(
      r#"
      .test {
        animation: var(--animation);
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        animation: var(--animation);
      }
    "#},
      map! {
        "test" => "EgL3uq_test"
      },
      HashMap::new(),
      Default::default(),
      false,
    );
    css_modules_test(
      r#"
      .test {
        animation: rotate var(--duration);
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        animation: rotate var(--duration);
      }
    "#},
      map! {
        "test" => "EgL3uq_test"
      },
      HashMap::new(),
      crate::css_modules::Config {
        animation: false,
        ..Default::default()
      },
      false,
    );
    css_modules_test(
      r#"
      .test {
        animation: "rotate" var(--duration);
      }
    "#,
      indoc! {r#"
      .EgL3uq_test {
        animation: EgL3uq_rotate var(--duration);
      }
    "#},
      map! {
        "test" => "EgL3uq_test",
        "rotate" => "EgL3uq_rotate" referenced: true
      },
      HashMap::new(),
      crate::css_modules::Config { ..Default::default() },
      false,
    );

    css_modules_test(
      r#"
      .test {
        composes: foo bar from "foo.css";
        background: white;
      }
    "#,
      indoc! {r#"
      ._5h2kwG-test {
        background: #fff;
      }
    "#},
      map! {
        "test" => "_5h2kwG-test" "foo" from "foo.css" "bar" from "foo.css"
      },
      HashMap::new(),
      crate::css_modules::Config {
        pattern: crate::css_modules::Pattern::parse("[content-hash]-[local]").unwrap(),
        ..Default::default()
      },
      false,
    );

    css_modules_test(
      r#"
      .box2 {
        @container main (width >= 0) {
          background-color: #90ee90;
        }
      }
    "#,
      indoc! {r#"
      .EgL3uq_box2 {
        @container EgL3uq_main (width >= 0) {
          & {
            background-color: #90ee90;
          }
        }
      }
    "#},
      map! {
        "main" => "EgL3uq_main",
        "box2" => "EgL3uq_box2"
      },
      HashMap::new(),
      crate::css_modules::Config { ..Default::default() },
      false,
    );

    css_modules_test(
      r#"
      .box2 {
        @container main (width >= 0) {
          background-color: #90ee90;
        }
      }
    "#,
      indoc! {r#"
      .EgL3uq_box2 {
        @container main (width >= 0) {
          & {
            background-color: #90ee90;
          }
        }
      }
    "#},
      map! {
        "box2" => "EgL3uq_box2"
      },
      HashMap::new(),
      crate::css_modules::Config {
        container: false,
        ..Default::default()
      },
      false,
    );

    css_modules_test(
      ".foo { view-transition-name: bar }",
      ".EgL3uq_foo{view-transition-name:EgL3uq_bar}",
      map! {
        "foo" => "EgL3uq_foo",
        "bar" => "EgL3uq_bar"
      },
      HashMap::new(),
      Default::default(),
      true,
    );
    css_modules_test(
      ".foo { view-transition-name: none }",
      ".EgL3uq_foo{view-transition-name:none}",
      map! {
        "foo" => "EgL3uq_foo"
      },
      HashMap::new(),
      Default::default(),
      true,
    );
    css_modules_test(
      ".foo { view-transition-name: auto }",
      ".EgL3uq_foo{view-transition-name:auto}",
      map! {
        "foo" => "EgL3uq_foo"
      },
      HashMap::new(),
      Default::default(),
      true,
    );

    css_modules_test(
      ".foo { view-transition-class: bar baz qux }",
      ".EgL3uq_foo{view-transition-class:EgL3uq_bar EgL3uq_baz EgL3uq_qux}",
      map! {
        "foo" => "EgL3uq_foo",
        "bar" => "EgL3uq_bar",
        "baz" => "EgL3uq_baz",
        "qux" => "EgL3uq_qux"
      },
      HashMap::new(),
      Default::default(),
      true,
    );

    css_modules_test(
      ".foo { view-transition-group: contain }",
      ".EgL3uq_foo{view-transition-group:contain}",
      map! {
        "foo" => "EgL3uq_foo"
      },
      HashMap::new(),
      Default::default(),
      true,
    );
    css_modules_test(
      ".foo { view-transition-group: bar }",
      ".EgL3uq_foo{view-transition-group:EgL3uq_bar}",
      map! {
        "foo" => "EgL3uq_foo",
        "bar" => "EgL3uq_bar"
      },
      HashMap::new(),
      Default::default(),
      true,
    );

    css_modules_test(
      "@view-transition { types: foo bar baz }",
      "@view-transition{types:EgL3uq_foo EgL3uq_bar EgL3uq_baz}",
      map! {
        "foo" => "EgL3uq_foo",
        "bar" => "EgL3uq_bar",
        "baz" => "EgL3uq_baz"
      },
      HashMap::new(),
      Default::default(),
      true,
    );

    css_modules_test(
      ":root:active-view-transition-type(foo, bar) { color: red }",
      ":root:active-view-transition-type(EgL3uq_foo,EgL3uq_bar){color:red}",
      map! {
        "foo" => "EgL3uq_foo",
        "bar" => "EgL3uq_bar"
      },
      HashMap::new(),
      Default::default(),
      true,
    );

    for name in &[
      "view-transition-group",
      "view-transition-image-pair",
      "view-transition-new",
      "view-transition-old",
    ] {
      css_modules_test(
        &format!(":root::{}(foo) {{position: fixed}}", name),
        &format!(":root::{}(EgL3uq_foo){{position:fixed}}", name),
        map! {
          "foo" => "EgL3uq_foo"
        },
        HashMap::new(),
        Default::default(),
        true,
      );
      css_modules_test(
        &format!(":root::{}(.bar) {{position: fixed}}", name),
        &format!(":root::{}(.EgL3uq_bar){{position:fixed}}", name),
        map! {
          "bar" => "EgL3uq_bar"
        },
        HashMap::new(),
        Default::default(),
        true,
      );
      css_modules_test(
        &format!(":root::{}(foo.bar.baz) {{position: fixed}}", name),
        &format!(":root::{}(EgL3uq_foo.EgL3uq_bar.EgL3uq_baz){{position:fixed}}", name),
        map! {
          "foo" => "EgL3uq_foo",
          "bar" => "EgL3uq_bar",
          "baz" => "EgL3uq_baz"
        },
        HashMap::new(),
        Default::default(),
        true,
      );

      css_modules_test(
        ":nth-child(1 of .foo) {width: 20px}",
        ":nth-child(1 of .EgL3uq_foo){width:20px}",
        map! {
          "foo" => "EgL3uq_foo"
        },
        HashMap::new(),
        Default::default(),
        true,
      );
      css_modules_test(
        ":nth-last-child(1 of .foo) {width: 20px}",
        ":nth-last-child(1 of .EgL3uq_foo){width:20px}",
        map! {
          "foo" => "EgL3uq_foo"
        },
        HashMap::new(),
        Default::default(),
        true,
      );
    }

    // Stable hashes between project roots.
    fn test_project_root(project_root: &str, filename: &str, hash: &str) {
      let stylesheet = StyleSheet::parse(
        r#"
        .foo {
          background: red;
        }
        "#,
        ParserOptions {
          filename: filename.into(),
          css_modules: Some(Default::default()),
          ..ParserOptions::default()
        },
      )
      .unwrap();
      let res = stylesheet
        .to_css(PrinterOptions {
          project_root: Some(project_root),
          ..PrinterOptions::default()
        })
        .unwrap();
      assert_eq!(
        res.code,
        format!(
          indoc! {r#"
      .{}_foo {{
        background: red;
      }}
      "#},
          hash
        )
      );
    }

    test_project_root("/foo/bar", "/foo/bar/test.css", "EgL3uq");
    test_project_root("/foo", "/foo/test.css", "EgL3uq");
    test_project_root("/foo/bar", "/foo/bar/baz/test.css", "xLEkNW");
    test_project_root("/foo", "/foo/baz/test.css", "xLEkNW");
  }

  #[test]
  fn test_pseudo_replacement() {
    let source = r#"
      .foo:hover {
        color: red;
      }

      .foo:active {
        color: yellow;
      }

      .foo:focus-visible {
        color: purple;
      }
    "#;

    let expected = indoc! { r#"
      .foo.is-hovered {
        color: red;
      }

      .foo.is-active {
        color: #ff0;
      }

      .foo.focus-visible {
        color: purple;
      }
    "#};

    let stylesheet = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
    let res = stylesheet
      .to_css(PrinterOptions {
        pseudo_classes: Some(PseudoClasses {
          hover: Some("is-hovered"),
          active: Some("is-active"),
          focus_visible: Some("focus-visible"),
          ..PseudoClasses::default()
        }),
        ..PrinterOptions::default()
      })
      .unwrap();
    assert_eq!(res.code, expected);

    let source = r#"
      .foo:hover {
        color: red;
      }
    "#;

    let expected = indoc! { r#"
      .EgL3uq_foo.EgL3uq_is-hovered {
        color: red;
      }
    "#};

    let stylesheet = StyleSheet::parse(
      &source,
      ParserOptions {
        filename: "test.css".into(),
        css_modules: Some(Default::default()),
        ..ParserOptions::default()
      },
    )
    .unwrap();
    let res = stylesheet
      .to_css(PrinterOptions {
        pseudo_classes: Some(PseudoClasses {
          hover: Some("is-hovered"),
          ..PseudoClasses::default()
        }),
        ..PrinterOptions::default()
      })
      .unwrap();
    assert_eq!(res.code, expected);
  }

  #[test]
  fn test_unused_symbols() {
    let source = r#"
      .foo {
        color: red;
      }

      .bar {
        color: green;
      }

      .bar:hover {
        color: purple;
      }

      .bar .baz {
        background: red;
      }

      .baz:is(.bar) {
        background: green;
      }

      #id {
        animation: 2s test;
      }

      #other_id {
        color: red;
      }

      @keyframes test {
        from { color: red }
        to { color: yellow }
      }

      @counter-style circles {
        symbols: Ⓐ Ⓑ Ⓒ;
      }

      @keyframes fade {
        from { opacity: 0 }
        to { opacity: 1 }
      }
    "#;

    let expected = indoc! {r#"
      .foo {
        color: red;
      }

      #id {
        animation: 2s test;
      }

      @keyframes test {
        from {
          color: red;
        }

        to {
          color: #ff0;
        }
      }
    "#};

    let mut stylesheet = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
    stylesheet
      .minify(MinifyOptions {
        unused_symbols: vec!["bar", "other_id", "fade", "circles"]
          .iter()
          .map(|s| String::from(*s))
          .collect(),
        ..MinifyOptions::default()
      })
      .unwrap();
    let res = stylesheet.to_css(PrinterOptions::default()).unwrap();
    assert_eq!(res.code, expected);

    let source = r#"
      .foo {
        color: red;

        &.bar {
          color: green;
        }
      }
    "#;

    let expected = indoc! {r#"
      .foo {
        color: red;
      }
    "#};

    let mut stylesheet = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
    stylesheet
      .minify(MinifyOptions {
        unused_symbols: vec!["bar"].iter().map(|s| String::from(*s)).collect(),
        ..MinifyOptions::default()
      })
      .unwrap();
    let res = stylesheet.to_css(PrinterOptions::default()).unwrap();
    assert_eq!(res.code, expected);

    let source = r#"
      .foo {
        color: red;

        &.bar {
          color: purple;
        }

        @nest &.bar {
          color: orange;
        }

        @nest :not(&) {
          color: green;
        }

        @media (orientation: portrait) {
          color: brown;
        }
      }

      .x {
        color: purple;

        &.y {
          color: green;
        }
      }
    "#;

    let expected = indoc! {r#"
      :not(.foo) {
        color: green;
      }
    "#};

    let mut stylesheet = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
    stylesheet
      .minify(MinifyOptions {
        unused_symbols: vec!["foo", "x"].iter().map(|s| String::from(*s)).collect(),
        ..MinifyOptions::default()
      })
      .unwrap();
    let res = stylesheet
      .to_css(PrinterOptions {
        targets: Browsers {
          chrome: Some(95 << 16),
          ..Browsers::default()
        }
        .into(),
        ..PrinterOptions::default()
      })
      .unwrap();
    assert_eq!(res.code, expected);

    let source = r#"
      @property --EgL3uq_foo {
        syntax: "<color>";
        inherits: false;
        initial-value: #ff0;
      }

      @font-palette-values --EgL3uq_Cooler {
        font-family: Bixa;
        base-palette: 1;
        override-colors: 1 #7EB7E4;
      }

      .EgL3uq_foo {
        --EgL3uq_foo: red;
      }

      .EgL3uq_bar {
        color: green;
      }
    "#;

    let expected = indoc! {r#"
      .EgL3uq_bar {
        color: green;
      }
    "#};

    let mut stylesheet = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
    stylesheet
      .minify(MinifyOptions {
        unused_symbols: vec!["--EgL3uq_foo", "--EgL3uq_Cooler"]
          .iter()
          .map(|s| String::from(*s))
          .collect(),
        ..MinifyOptions::default()
      })
      .unwrap();
    let res = stylesheet.to_css(PrinterOptions::default()).unwrap();
    assert_eq!(res.code, expected);
  }

  #[test]
  fn test_svg() {
    minify_test(".foo { fill: yellow; }", ".foo{fill:#ff0}");
    minify_test(".foo { fill: url(#foo); }", ".foo{fill:url(#foo)}");
    minify_test(".foo { fill: url(#foo) none; }", ".foo{fill:url(#foo) none}");
    minify_test(".foo { fill: url(#foo) yellow; }", ".foo{fill:url(#foo) #ff0}");
    minify_test(".foo { fill: none; }", ".foo{fill:none}");
    minify_test(".foo { fill: context-fill; }", ".foo{fill:context-fill}");
    minify_test(".foo { fill: context-stroke; }", ".foo{fill:context-stroke}");

    minify_test(".foo { stroke: yellow; }", ".foo{stroke:#ff0}");
    minify_test(".foo { stroke: url(#foo); }", ".foo{stroke:url(#foo)}");
    minify_test(".foo { stroke: url(#foo) none; }", ".foo{stroke:url(#foo) none}");
    minify_test(".foo { stroke: url(#foo) yellow; }", ".foo{stroke:url(#foo) #ff0}");
    minify_test(".foo { stroke: none; }", ".foo{stroke:none}");
    minify_test(".foo { stroke: context-fill; }", ".foo{stroke:context-fill}");
    minify_test(".foo { stroke: context-stroke; }", ".foo{stroke:context-stroke}");

    minify_test(".foo { marker-start: url(#foo); }", ".foo{marker-start:url(#foo)}");

    minify_test(".foo { stroke-dasharray: 4 1 2; }", ".foo{stroke-dasharray:4 1 2}");
    minify_test(".foo { stroke-dasharray: 4,1,2; }", ".foo{stroke-dasharray:4 1 2}");
    minify_test(".foo { stroke-dasharray: 4, 1, 2; }", ".foo{stroke-dasharray:4 1 2}");
    minify_test(
      ".foo { stroke-dasharray: 4px, 1px, 2px; }",
      ".foo{stroke-dasharray:4 1 2}",
    );

    minify_test(".foo { mask: url('foo.svg'); }", ".foo{mask:url(foo.svg)}");
    minify_test(
      ".foo { mask: url(masks.svg#star) luminance }",
      ".foo{mask:url(masks.svg#star) luminance}",
    );
    minify_test(
      ".foo { mask: url(masks.svg#star) 40px 20px }",
      ".foo{mask:url(masks.svg#star) 40px 20px}",
    );
    minify_test(
      ".foo { mask: url(masks.svg#star) 0 0 / 50px 50px }",
      ".foo{mask:url(masks.svg#star) 0 0/50px 50px}",
    );
    minify_test(
      ".foo { mask: url(masks.svg#star) repeat-x }",
      ".foo{mask:url(masks.svg#star) repeat-x}",
    );
    minify_test(
      ".foo { mask: url(masks.svg#star) stroke-box }",
      ".foo{mask:url(masks.svg#star) stroke-box}",
    );
    minify_test(
      ".foo { mask: url(masks.svg#star) stroke-box stroke-box }",
      ".foo{mask:url(masks.svg#star) stroke-box}",
    );
    minify_test(
      ".foo { mask: url(masks.svg#star) border-box }",
      ".foo{mask:url(masks.svg#star)}",
    );
    minify_test(
      ".foo { mask: url(masks.svg#star) left / 16px repeat-y, url(masks.svg#circle) right / 16px repeat-y }",
      ".foo{mask:url(masks.svg#star) 0/16px repeat-y,url(masks.svg#circle) 100%/16px repeat-y}",
    );

    minify_test(
      ".foo { mask-border: url('border-mask.png') 25; }",
      ".foo{mask-border:url(border-mask.png) 25}",
    );
    minify_test(
      ".foo { mask-border: url('border-mask.png') 25 / 35px / 12px space alpha; }",
      ".foo{mask-border:url(border-mask.png) 25/35px/12px space}",
    );
    minify_test(
      ".foo { mask-border: url('border-mask.png') 25 / 35px / 12px space luminance; }",
      ".foo{mask-border:url(border-mask.png) 25/35px/12px space luminance}",
    );
    minify_test(
      ".foo { mask-border: url('border-mask.png') luminance 25 / 35px / 12px space; }",
      ".foo{mask-border:url(border-mask.png) 25/35px/12px space luminance}",
    );

    minify_test(
      ".foo { clip-path: url('clip.svg#star'); }",
      ".foo{clip-path:url(clip.svg#star)}",
    );
    minify_test(".foo { clip-path: margin-box; }", ".foo{clip-path:margin-box}");
    minify_test(
      ".foo { clip-path: inset(100px 50px); }",
      ".foo{clip-path:inset(100px 50px)}",
    );
    minify_test(
      ".foo { clip-path: inset(100px 50px round 5px); }",
      ".foo{clip-path:inset(100px 50px round 5px)}",
    );
    minify_test(
      ".foo { clip-path: inset(100px 50px round 5px 5px 5px 5px); }",
      ".foo{clip-path:inset(100px 50px round 5px)}",
    );
    minify_test(".foo { clip-path: circle(50px); }", ".foo{clip-path:circle(50px)}");
    minify_test(
      ".foo { clip-path: circle(50px at center center); }",
      ".foo{clip-path:circle(50px)}",
    );
    minify_test(
      ".foo { clip-path: circle(50px at 50% 50%); }",
      ".foo{clip-path:circle(50px)}",
    );
    minify_test(
      ".foo { clip-path: circle(50px at 0 100px); }",
      ".foo{clip-path:circle(50px at 0 100px)}",
    );
    minify_test(
      ".foo { clip-path: circle(closest-side at 0 100px); }",
      ".foo{clip-path:circle(at 0 100px)}",
    );
    minify_test(
      ".foo { clip-path: circle(farthest-side at 0 100px); }",
      ".foo{clip-path:circle(farthest-side at 0 100px)}",
    );
    minify_test(
      ".foo { clip-path: circle(closest-side at 50% 50%); }",
      ".foo{clip-path:circle()}",
    );
    minify_test(
      ".foo { clip-path: ellipse(50px 60px at 0 10% 20%); }",
      ".foo{clip-path:ellipse(50px 60px at 0 10% 20%)}",
    );
    minify_test(
      ".foo { clip-path: ellipse(50px 60px at center center); }",
      ".foo{clip-path:ellipse(50px 60px)}",
    );
    minify_test(
      ".foo { clip-path: ellipse(closest-side closest-side at 50% 50%); }",
      ".foo{clip-path:ellipse()}",
    );
    minify_test(
      ".foo { clip-path: ellipse(closest-side closest-side at 10% 20%); }",
      ".foo{clip-path:ellipse(at 10% 20%)}",
    );
    minify_test(
      ".foo { clip-path: ellipse(farthest-side closest-side at 10% 20%); }",
      ".foo{clip-path:ellipse(farthest-side closest-side at 10% 20%)}",
    );
    minify_test(
      ".foo { clip-path: polygon(50% 0%, 100% 50%, 50% 100%, 0% 50%); }",
      ".foo{clip-path:polygon(50% 0%,100% 50%,50% 100%,0% 50%)}",
    );
    minify_test(
      ".foo { clip-path: polygon(nonzero, 50% 0%, 100% 50%, 50% 100%, 0% 50%); }",
      ".foo{clip-path:polygon(50% 0%,100% 50%,50% 100%,0% 50%)}",
    );
    minify_test(
      ".foo { clip-path: polygon(evenodd, 50% 0%, 100% 50%, 50% 100%, 0% 50%); }",
      ".foo{clip-path:polygon(evenodd,50% 0%,100% 50%,50% 100%,0% 50%)}",
    );
    minify_test(
      ".foo { clip-path: padding-box circle(50px at 0 100px); }",
      ".foo{clip-path:circle(50px at 0 100px) padding-box}",
    );
    minify_test(
      ".foo { clip-path: circle(50px at 0 100px) padding-box; }",
      ".foo{clip-path:circle(50px at 0 100px) padding-box}",
    );
    minify_test(
      ".foo { clip-path: circle(50px at 0 100px) border-box; }",
      ".foo{clip-path:circle(50px at 0 100px)}",
    );

    prefix_test(
      ".foo { clip-path: circle(50px); }",
      indoc! { r#"
        .foo {
          -webkit-clip-path: circle(50px);
          clip-path: circle(50px);
        }
      "#},
      Browsers {
        chrome: Some(30 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { clip-path: circle(50px); }",
      indoc! { r#"
        .foo {
          clip-path: circle(50px);
        }
      "#},
      Browsers {
        chrome: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { clip-path: circle(50px); }",
      indoc! { r#"
        .foo {
          -webkit-clip-path: circle(50px);
          clip-path: circle(50px);
        }
      "#},
      Browsers {
        safari: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { clip-path: circle(50px); }",
      indoc! { r#"
        .foo {
          clip-path: circle(50px);
        }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { fill: lch(50.998% 135.363 338) }",
      indoc! { r#"
        .foo {
          fill: #ee00be;
          fill: color(display-p3 .972962 -.362078 .804206);
          fill: lch(50.998% 135.363 338);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { stroke: lch(50.998% 135.363 338) }",
      indoc! { r#"
        .foo {
          stroke: #ee00be;
          stroke: color(display-p3 .972962 -.362078 .804206);
          stroke: lch(50.998% 135.363 338);
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { fill: url(#foo) lch(50.998% 135.363 338) }",
      indoc! { r##"
        .foo {
          fill: url("#foo") #ee00be;
          fill: url("#foo") color(display-p3 .972962 -.362078 .804206);
          fill: url("#foo") lch(50.998% 135.363 338);
        }
      "##},
      Browsers {
        chrome: Some(90 << 16),
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { fill: var(--url) lch(50.998% 135.363 338) }",
      indoc! { r#"
        .foo {
          fill: var(--url) #ee00be;
        }

        @supports (color: lab(0% 0 0)) {
          .foo {
            fill: var(--url) lab(50.998% 125.506 -50.7078);
          }
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { mask-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          -webkit-mask-image: -webkit-gradient(linear, 0 0, 0 100%, from(#ff0f0e), to(#7773ff));
          -webkit-mask-image: -webkit-linear-gradient(#ff0f0e, #7773ff);
          -webkit-mask-image: linear-gradient(#ff0f0e, #7773ff);
          mask-image: linear-gradient(#ff0f0e, #7773ff);
          -webkit-mask-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
          mask-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { mask-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) }",
      indoc! { r#"
        .foo {
          -webkit-mask-image: linear-gradient(#ff0f0e, #7773ff);
          mask-image: linear-gradient(#ff0f0e, #7773ff);
          -webkit-mask-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
          mask-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { mask-image: linear-gradient(red, green) }",
      indoc! { r#"
        .foo {
          -webkit-mask-image: linear-gradient(red, green);
          mask-image: linear-gradient(red, green);
        }
      "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { -webkit-mask-image: url(x.svg); mask-image: url(x.svg); }",
      indoc! { r#"
        .foo {
          -webkit-mask-image: url("x.svg");
          mask-image: url("x.svg");
        }
      "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { mask: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 40px 20px }",
      indoc! { r#"
        .foo {
          -webkit-mask: -webkit-gradient(linear, 0 0, 0 100%, from(#ff0f0e), to(#7773ff)) 40px 20px;
          -webkit-mask: -webkit-linear-gradient(#ff0f0e, #7773ff) 40px 20px;
          -webkit-mask: linear-gradient(#ff0f0e, #7773ff) 40px 20px;
          mask: linear-gradient(#ff0f0e, #7773ff) 40px 20px;
          -webkit-mask: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 40px 20px;
          mask: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 40px 20px;
        }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { mask: -webkit-linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 40px 20px }",
      indoc! { r#"
        .foo {
          -webkit-mask: -webkit-gradient(linear, 0 0, 0 100%, from(#ff0f0e), to(#7773ff)) 40px 20px;
          -webkit-mask: -webkit-linear-gradient(#ff0f0e, #7773ff) 40px 20px;
        }
      "#},
      Browsers {
        chrome: Some(8 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { mask: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 40px var(--foo) }",
      indoc! { r#"
        .foo {
          -webkit-mask: linear-gradient(#ff0f0e, #7773ff) 40px var(--foo);
          mask: linear-gradient(#ff0f0e, #7773ff) 40px var(--foo);
        }

        @supports (color: lab(0% 0 0)) {
          .foo {
            -webkit-mask: linear-gradient(lab(56.208% 94.4644 98.8928), lab(51% 70.4544 -115.586)) 40px var(--foo);
            mask: linear-gradient(lab(56.208% 94.4644 98.8928), lab(51% 70.4544 -115.586)) 40px var(--foo);
          }
        }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { mask: url(masks.svg#star) luminance }",
      indoc! { r#"
        .foo {
          -webkit-mask: url("masks.svg#star");
          -webkit-mask-source-type: luminance;
          mask: url("masks.svg#star") luminance;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { mask-image: url(masks.svg#star) }",
      indoc! { r#"
        .foo {
          -webkit-mask-image: url("masks.svg#star");
          mask-image: url("masks.svg#star");
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          mask-image: url(masks.svg#star);
          mask-position: 25% 75%;
          mask-size: cover;
          mask-repeat: no-repeat;
          mask-clip: padding-box;
          mask-origin: content-box;
          mask-composite: subtract;
          mask-mode: luminance;
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask: url("masks.svg#star") 25% 75% / cover no-repeat content-box padding-box;
          -webkit-mask-composite: source-out;
          -webkit-mask-source-type: luminance;
          mask: url("masks.svg#star") 25% 75% / cover no-repeat content-box padding-box subtract luminance;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          mask-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
          mask-position: 25% 75%;
          mask-size: cover;
          mask-repeat: no-repeat;
          mask-clip: padding-box;
          mask-origin: content-box;
          mask-composite: subtract;
          mask-mode: luminance;
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask: linear-gradient(#ff0f0e, #7773ff) 25% 75% / cover no-repeat content-box padding-box;
          -webkit-mask-composite: source-out;
          -webkit-mask-source-type: luminance;
          mask: linear-gradient(#ff0f0e, #7773ff) 25% 75% / cover no-repeat content-box padding-box subtract luminance;
          -webkit-mask: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 25% 75% / cover no-repeat content-box padding-box;
          -webkit-mask-composite: source-out;
          -webkit-mask-source-type: luminance;
          mask: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 25% 75% / cover no-repeat content-box padding-box subtract luminance;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    test(
      r#"
        .foo {
          mask: none center / 100% no-repeat;
          mask-image: var(--svg);
        }
      "#,
      indoc! { r#"
        .foo {
          mask: none center / 100% no-repeat;
          mask-image: var(--svg);
        }
      "#},
    );

    prefix_test(
      r#"
        .foo {
          mask-composite: subtract;
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask-composite: source-out;
          mask-composite: subtract;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          mask-mode: luminance;
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask-source-type: luminance;
          mask-mode: luminance;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          mask-border: url('border-mask.png') 25 / 35px / 12px space luminance;
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask-box-image: url("border-mask.png") 25 / 35px / 12px space;
          mask-border: url("border-mask.png") 25 / 35px / 12px space luminance;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          mask-border: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 25 / 35px / 12px space luminance;
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask-box-image: linear-gradient(#ff0f0e, #7773ff) 25 / 35px / 12px space;
          mask-border: linear-gradient(#ff0f0e, #7773ff) 25 / 35px / 12px space luminance;
          -webkit-mask-box-image: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 25 / 35px / 12px space;
          mask-border: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) 25 / 35px / 12px space luminance;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          mask-border-source: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask-box-image-source: linear-gradient(#ff0f0e, #7773ff);
          mask-border-source: linear-gradient(#ff0f0e, #7773ff);
          -webkit-mask-box-image-source: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
          mask-border-source: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364));
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          mask-border-source: url(foo.png);
          mask-border-slice: 10 40 10 40;
          mask-border-width: 10px;
          mask-border-outset: 0;
          mask-border-repeat: round round;
          mask-border-mode: luminance;
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask-box-image: url("foo.png") 10 40 / 10px round;
          mask-border: url("foo.png") 10 40 / 10px round luminance;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          -webkit-mask-box-image-source: url(foo.png);
          -webkit-mask-box-image-slice: 10 40 10 40;
          -webkit-mask-box-image-width: 10px;
          -webkit-mask-box-image-outset: 0;
          -webkit-mask-box-image-repeat: round round;
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask-box-image: url("foo.png") 10 40 / 10px round;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          mask-border-slice: 10 40 10 40;
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask-box-image-slice: 10 40;
          mask-border-slice: 10 40;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          mask-border-slice: var(--foo);
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask-box-image-slice: var(--foo);
          mask-border-slice: var(--foo);
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          mask-border: linear-gradient(lch(56.208% 136.76 46.312), lch(51% 135.366 301.364)) var(--foo);
        }
      "#,
      indoc! { r#"
        .foo {
          -webkit-mask-box-image: linear-gradient(#ff0f0e, #7773ff) var(--foo);
          mask-border: linear-gradient(#ff0f0e, #7773ff) var(--foo);
        }

        @supports (color: lab(0% 0 0)) {
          .foo {
            -webkit-mask-box-image: linear-gradient(lab(56.208% 94.4644 98.8928), lab(51% 70.4544 -115.586)) var(--foo);
            mask-border: linear-gradient(lab(56.208% 94.4644 98.8928), lab(51% 70.4544 -115.586)) var(--foo);
          }
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          transition: mask 200ms;
        }
      "#,
      indoc! { r#"
        .foo {
          transition: -webkit-mask .2s, mask .2s;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          transition: mask-border 200ms;
        }
      "#,
      indoc! { r#"
        .foo {
          transition: -webkit-mask-box-image .2s, mask-border .2s;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          transition-property: mask;
        }
      "#,
      indoc! { r#"
        .foo {
          transition-property: -webkit-mask, mask;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          transition-property: mask-border;
        }
      "#,
      indoc! { r#"
        .foo {
          transition-property: -webkit-mask-box-image, mask-border;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
        .foo {
          transition-property: mask-composite, mask-mode;
        }
      "#,
      indoc! { r#"
        .foo {
          transition-property: -webkit-mask-composite, mask-composite, -webkit-mask-source-type, mask-mode;
        }
    "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_filter() {
    minify_test(
      ".foo { filter: url('filters.svg#filter-id'); }",
      ".foo{filter:url(filters.svg#filter-id)}",
    );
    minify_test(".foo { filter: blur(5px); }", ".foo{filter:blur(5px)}");
    minify_test(".foo { filter: blur(0px); }", ".foo{filter:blur()}");
    minify_test(".foo { filter: brightness(10%); }", ".foo{filter:brightness(10%)}");
    minify_test(".foo { filter: brightness(100%); }", ".foo{filter:brightness()}");
    minify_test(
      ".foo { filter: drop-shadow(16px 16px 20px yellow); }",
      ".foo{filter:drop-shadow(16px 16px 20px #ff0)}",
    );
    minify_test(
      ".foo { filter: contrast(175%) brightness(3%); }",
      ".foo{filter:contrast(175%)brightness(3%)}",
    );
    minify_test(".foo { filter: hue-rotate(0) }", ".foo{filter:hue-rotate()}");

    prefix_test(
      ".foo { filter: blur(5px) }",
      indoc! { r#"
        .foo {
          -webkit-filter: blur(5px);
          filter: blur(5px);
        }
      "#},
      Browsers {
        chrome: Some(20 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { filter: blur(5px) }",
      indoc! { r#"
        .foo {
          filter: blur(5px);
        }
      "#},
      Browsers {
        chrome: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { backdrop-filter: blur(5px) }",
      indoc! { r#"
        .foo {
          backdrop-filter: blur(5px);
        }
      "#},
      Browsers {
        chrome: Some(80 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { backdrop-filter: blur(5px) }",
      indoc! { r#"
        .foo {
          -webkit-backdrop-filter: blur(5px);
          backdrop-filter: blur(5px);
        }
      "#},
      Browsers {
        safari: Some(15 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        -webkit-backdrop-filter: blur(8px);
        backdrop-filter: blur(8px);
      }
      "#,
      indoc! {r#"
      .foo {
        -webkit-backdrop-filter: blur(8px);
        backdrop-filter: blur(8px);
      }
      "#},
      Browsers {
        safari: Some(16 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { filter: var(--foo) }",
      indoc! { r#"
        .foo {
          -webkit-filter: var(--foo);
          filter: var(--foo);
        }
      "#},
      Browsers {
        chrome: Some(20 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { filter: drop-shadow(16px 16px 20px lab(40% 56.6 39)) }",
      indoc! { r#"
        .foo {
          -webkit-filter: drop-shadow(16px 16px 20px #b32323);
          filter: drop-shadow(16px 16px 20px #b32323);
          filter: drop-shadow(16px 16px 20px lab(40% 56.6 39));
        }
      "#},
      Browsers {
        chrome: Some(20 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { filter: contrast(175%) drop-shadow(16px 16px 20px lab(40% 56.6 39)) }",
      indoc! { r#"
        .foo {
          filter: contrast(175%) drop-shadow(16px 16px 20px #b32323);
          filter: contrast(175%) drop-shadow(16px 16px 20px lab(40% 56.6 39));
        }
      "#},
      Browsers {
        chrome: Some(4 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { filter: drop-shadow(16px 16px 20px lab(40% 56.6 39)) drop-shadow(16px 16px 20px yellow) }",
      indoc! { r#"
        .foo {
          filter: drop-shadow(16px 16px 20px #b32323) drop-shadow(16px 16px 20px #ff0);
          filter: drop-shadow(16px 16px 20px lab(40% 56.6 39)) drop-shadow(16px 16px 20px #ff0);
        }
      "#},
      Browsers {
        chrome: Some(4 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { filter: var(--foo) drop-shadow(16px 16px 20px lab(40% 56.6 39)) }",
      indoc! { r#"
        .foo {
          filter: var(--foo) drop-shadow(16px 16px 20px #b32323);
        }

        @supports (color: lab(0% 0 0)) {
          .foo {
            filter: var(--foo) drop-shadow(16px 16px 20px lab(40% 56.6 39));
          }
        }
      "#},
      Browsers {
        chrome: Some(4 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_viewport() {
    minify_test(
      r#"
    @viewport {
      width: 100vw;
    }"#,
      "@viewport{width:100vw}",
    );
    minify_test(
      r#"
    @-ms-viewport {
      width: device-width;
    }"#,
      "@-ms-viewport{width:device-width}",
    );
  }

  #[test]
  fn test_at_scope() {
    minify_test(
      r#"
      @scope {
        .foo {
          display: flex;
        }
      }
      "#,
      "@scope{.foo{display:flex}}",
    );
    minify_test(
      r#"
      @scope {
        :scope {
          display: flex;
          color: lightblue;
        }
      }"#,
      "@scope{:scope{color:#add8e6;display:flex}}",
    );
    minify_test(
      r#"
      @scope (.light-scheme) {
        a { color: yellow; }
      }
      "#,
      "@scope(.light-scheme){a{color:#ff0}}",
    );
    minify_test(
      r#"
      @scope (.media-object) to (.content > *) {
        a { color: yellow; }
      }
      "#,
      "@scope(.media-object) to (.content>*){a{color:#ff0}}",
    );
    minify_test(
      r#"
      @scope to (.content > *) {
        a { color: yellow; }
      }
      "#,
      "@scope to (.content>*){a{color:#ff0}}",
    );
    minify_test(
      r#"
      @scope (#my-component) {
        & { color: yellow; }
      }
      "#,
      "@scope(#my-component){&{color:#ff0}}",
    );
    minify_test(
      r#"
      @scope (.parent-scope) {
        @scope (:scope > .child-scope) to (:scope .limit) {
          .content { color: yellow; }
        }
      }
      "#,
      "@scope(.parent-scope){@scope(:scope>.child-scope) to (:scope .limit){.content{color:#ff0}}}",
    );
    minify_test(
      r#"
      .foo {
        @scope (.bar) {
          color: yellow;
        }
      }
      "#,
      ".foo{@scope(.bar){&{color:#ff0}}}",
    );
    nesting_test(
      r#"
      .foo {
        @scope (.bar) {
          color: yellow;
        }
      }
      "#,
      indoc! {r#"
        @scope (.bar) {
          :scope {
            color: #ff0;
          }
        }
      "#},
    );
    nesting_test(
      r#"
      .parent {
        color: blue;

        @scope (& > .scope) to (& .limit) {
          & .content {
            color: yellow;
          }
        }
      }
      "#,
      indoc! {r#"
        .parent {
          color: #00f;
        }

        @scope (.parent > .scope) to (.parent > .scope .limit) {
          :scope .content {
            color: #ff0;
          }
        }
      "#},
    );
  }

  #[test]
  fn test_custom_media() {
    custom_media_test(
      r#"
      @custom-media --modern (color), (hover);

      @media (--modern) and (width > 1024px) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media ((color) or (hover)) and (width > 1024px) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --color (color);

      @media (--color) and (width > 1024px) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media (color) and (width > 1024px) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --a (color);
      @custom-media --b (--a);

      @media (--b) and (width > 1024px) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media (color) and (width > 1024px) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --not-color not (color);

      @media not (--not-color) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media (color) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --color-print print and (color);

      @media (--color-print) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media print and (color) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --color-print print and (color);

      @media print and (--color-print) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media print and (color) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --not-color-print not print and (color);

      @media not print and (--not-color-print) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media not print and (color) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --print print;

      @media (--print) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media print {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --print print;

      @media not (--print) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media not print {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --print not print;

      @media not (--print) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media print {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --print print;

      @media ((--print)) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media print {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --color (color);
      @custom-media --print print;

      @media (--print) and (--color) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media print and (color) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --color (color);
      @custom-media --not-print not print;

      @media (--not-print) and (--color) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media not print and (color) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --color (color);
      @custom-media --screen screen;
      @custom-media --print print;

      @media (--print) and (--color), (--screen) and (--color) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media print and (color), screen and (color) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --color print and (color), print and (script);

      @media (--color) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
      @media print and ((color) or (script)) {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --color (color);
      @custom-media --not-color not all and (--color);

      @media (--not-color) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
        @media not all and (color) {
          .a {
            color: green;
          }
        }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --color (color);

      @media not all and (--color) {
        .a {
          color: green;
        }
      }
      "#,
      indoc! {r#"
        @media not all and (color) {
          .a {
            color: green;
          }
        }
      "#},
    );

    custom_media_test(
      r#"
      @media (--print) {
        .a {
          color: green;
        }
      }

      @custom-media --print print;
      "#,
      indoc! {r#"
      @media print {
        .a {
          color: green;
        }
      }
      "#},
    );

    custom_media_test(
      r#"
      @custom-media --not-width not (min-width: 300px);
      @media screen and ((prefers-color-scheme: dark) or (--not-width)) {
        .foo {
          order: 6;
        }
      }
      "#,
      indoc! {r#"
      @media screen and ((prefers-color-scheme: dark) or ((width < 300px))) {
        .foo {
          order: 6;
        }
      }
      "#},
    );

    fn custom_media_error_test(source: &str, err: Error<MinifyErrorKind>) {
      let mut stylesheet = StyleSheet::parse(
        &source,
        ParserOptions {
          filename: "test.css".into(),
          flags: ParserFlags::CUSTOM_MEDIA,
          ..ParserOptions::default()
        },
      )
      .unwrap();
      let res = stylesheet.minify(MinifyOptions {
        targets: Browsers {
          chrome: Some(95 << 16),
          ..Browsers::default()
        }
        .into(),
        ..MinifyOptions::default()
      });
      assert_eq!(res, Err(err))
    }

    custom_media_error_test(
      r#"
      @custom-media --color-print print and (color);

      @media screen and (--color-print) {
        .a {
          color: green;
        }
      }
      "#,
      Error {
        kind: MinifyErrorKind::UnsupportedCustomMediaBooleanLogic {
          custom_media_loc: Location {
            source_index: 0,
            line: 1,
            column: 7,
          },
        },
        loc: Some(ErrorLocation {
          filename: "test.css".into(),
          line: 3,
          column: 7,
        }),
      },
    );

    custom_media_error_test(
      r#"
      @custom-media --color-print print and (color);

      @media not print and (--color-print) {
        .a {
          color: green;
        }
      }
      "#,
      Error {
        kind: MinifyErrorKind::UnsupportedCustomMediaBooleanLogic {
          custom_media_loc: Location {
            source_index: 0,
            line: 1,
            column: 7,
          },
        },
        loc: Some(ErrorLocation {
          filename: "test.css".into(),
          line: 3,
          column: 7,
        }),
      },
    );

    custom_media_error_test(
      r#"
      @custom-media --color-print print and (color);
      @custom-media --color-screen screen and (color);

      @media (--color-print) or (--color-screen) {}
      "#,
      Error {
        kind: MinifyErrorKind::UnsupportedCustomMediaBooleanLogic {
          custom_media_loc: Location {
            source_index: 0,
            line: 2,
            column: 7,
          },
        },
        loc: Some(ErrorLocation {
          filename: "test.css".into(),
          line: 4,
          column: 7,
        }),
      },
    );

    custom_media_error_test(
      r#"
      @custom-media --color-print print and (color);
      @custom-media --color-screen screen and (color);

      @media (--color-print) and (--color-screen) {}
      "#,
      Error {
        kind: MinifyErrorKind::UnsupportedCustomMediaBooleanLogic {
          custom_media_loc: Location {
            source_index: 0,
            line: 2,
            column: 7,
          },
        },
        loc: Some(ErrorLocation {
          filename: "test.css".into(),
          line: 4,
          column: 7,
        }),
      },
    );

    custom_media_error_test(
      r#"
      @custom-media --screen screen;
      @custom-media --print print;

      @media (--print) and (--screen) {}
      "#,
      Error {
        kind: MinifyErrorKind::UnsupportedCustomMediaBooleanLogic {
          custom_media_loc: Location {
            source_index: 0,
            line: 1,
            column: 7,
          },
        },
        loc: Some(ErrorLocation {
          filename: "test.css".into(),
          line: 4,
          column: 7,
        }),
      },
    );

    custom_media_error_test(
      r#"
      @custom-media --not-print not print and (color);
      @custom-media --not-screen not screen and (color);

      @media ((script) or ((--not-print) and (--not-screen))) {
        .a {
          color: green;
        }
      }
      "#,
      Error {
        kind: MinifyErrorKind::UnsupportedCustomMediaBooleanLogic {
          custom_media_loc: Location {
            source_index: 0,
            line: 2,
            column: 7,
          },
        },
        loc: Some(ErrorLocation {
          filename: "test.css".into(),
          line: 4,
          column: 7,
        }),
      },
    );

    custom_media_error_test(
      r#"
      @custom-media --color screen and (color), print and (color);

      @media (--color) {
        .a {
          color: green;
        }
      }
      "#,
      Error {
        kind: MinifyErrorKind::UnsupportedCustomMediaBooleanLogic {
          custom_media_loc: Location {
            source_index: 0,
            line: 1,
            column: 7,
          },
        },
        loc: Some(ErrorLocation {
          filename: "test.css".into(),
          line: 3,
          column: 7,
        }),
      },
    );

    custom_media_error_test(
      r#"
      @media (--not-defined) {
        .a {
          color: green;
        }
      }
      "#,
      Error {
        kind: MinifyErrorKind::CustomMediaNotDefined {
          name: "--not-defined".into(),
        },
        loc: Some(ErrorLocation {
          filename: "test.css".into(),
          line: 1,
          column: 7,
        }),
      },
    );

    custom_media_error_test(
      r#"
      @custom-media --circular-mq-a (--circular-mq-b);
      @custom-media --circular-mq-b (--circular-mq-a);

      @media (--circular-mq-a) {
        body {
          order: 3;
        }
      }
      "#,
      Error {
        kind: MinifyErrorKind::CircularCustomMedia {
          name: "--circular-mq-a".into(),
        },
        loc: Some(ErrorLocation {
          filename: "test.css".into(),
          line: 4,
          column: 7,
        }),
      },
    );
  }

  #[test]
  fn test_dependencies() {
    fn dep_test(source: &str, expected: &str, deps: Vec<(&str, &str)>) {
      let mut stylesheet = StyleSheet::parse(
        &source,
        ParserOptions {
          filename: "test.css".into(),
          ..ParserOptions::default()
        },
      )
      .unwrap();
      stylesheet.minify(MinifyOptions::default()).unwrap();
      let res = stylesheet
        .to_css(PrinterOptions {
          analyze_dependencies: Some(Default::default()),
          minify: true,
          ..PrinterOptions::default()
        })
        .unwrap();
      assert_eq!(res.code, expected);
      let dependencies = res.dependencies.unwrap();
      assert_eq!(dependencies.len(), deps.len());
      for (i, (url, placeholder)) in deps.into_iter().enumerate() {
        match &dependencies[i] {
          Dependency::Url(dep) => {
            assert_eq!(dep.url, url);
            assert_eq!(dep.placeholder, placeholder);
          }
          Dependency::Import(dep) => {
            assert_eq!(dep.url, url);
            assert_eq!(dep.placeholder, placeholder);
          }
        }
      }
    }

    fn dep_error_test(source: &str, error: PrinterErrorKind) {
      let stylesheet = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
      let res = stylesheet.to_css(PrinterOptions {
        analyze_dependencies: Some(Default::default()),
        ..PrinterOptions::default()
      });
      match res {
        Err(e) => assert_eq!(e.kind, error),
        _ => unreachable!(),
      }
    }

    dep_test(
      ".foo { background: image-set('./img12x.png', './img21x.png' 2x)}",
      ".foo{background:image-set(\"hXFI8W\" 1x,\"5TkpBa\" 2x)}",
      vec![("./img12x.png", "hXFI8W"), ("./img21x.png", "5TkpBa")],
    );

    dep_test(
      ".foo { background: image-set(url(./img12x.png), url('./img21x.png') 2x)}",
      ".foo{background:image-set(\"hXFI8W\" 1x,\"5TkpBa\" 2x)}",
      vec![("./img12x.png", "hXFI8W"), ("./img21x.png", "5TkpBa")],
    );

    dep_test(
      ".foo { --test: url(/foo.png) }",
      ".foo{--test:url(\"lDnnrG\")}",
      vec![("/foo.png", "lDnnrG")],
    );

    dep_test(
      ".foo { --test: url(\"/foo.png\") }",
      ".foo{--test:url(\"lDnnrG\")}",
      vec![("/foo.png", "lDnnrG")],
    );

    dep_test(
      ".foo { --test: url(\"http://example.com/foo.png\") }",
      ".foo{--test:url(\"3X1zSW\")}",
      vec![("http://example.com/foo.png", "3X1zSW")],
    );

    dep_test(
      ".foo { --test: url(\"data:image/svg+xml;utf8,<svg></svg>\") }",
      ".foo{--test:url(\"-vl-rG\")}",
      vec![("data:image/svg+xml;utf8,<svg></svg>", "-vl-rG")],
    );

    dep_test(
      ".foo { background: url(\"foo.png\") var(--test) }",
      ".foo{background:url(\"Vwkwkq\") var(--test)}",
      vec![("foo.png", "Vwkwkq")],
    );

    dep_error_test(
      ".foo { --test: url(\"foo.png\") }",
      PrinterErrorKind::AmbiguousUrlInCustomProperty { url: "foo.png".into() },
    );

    dep_error_test(
      ".foo { --test: url(foo.png) }",
      PrinterErrorKind::AmbiguousUrlInCustomProperty { url: "foo.png".into() },
    );

    dep_error_test(
      ".foo { --test: url(./foo.png) }",
      PrinterErrorKind::AmbiguousUrlInCustomProperty {
        url: "./foo.png".into(),
      },
    );

    dep_test(
      ".foo { behavior: url(#foo) }",
      ".foo{behavior:url(\"Zn9-2q\")}",
      vec![("#foo", "Zn9-2q")],
    );

    dep_test(
      ".foo { --foo: url(#foo) }",
      ".foo{--foo:url(\"Zn9-2q\")}",
      vec![("#foo", "Zn9-2q")],
    );

    dep_test(
      "@import \"test.css\"; .foo { color: red }",
      "@import \"hHsogW\";.foo{color:red}",
      vec![("test.css", "hHsogW")],
    );
  }

  #[test]
  fn test_api() {
    let stylesheet = StyleSheet::parse(".foo:hover { color: red }", ParserOptions::default()).unwrap();
    match &stylesheet.rules.0[0] {
      CssRule::Style(s) => {
        assert_eq!(&s.selectors.to_string(), ".foo:hover");
      }
      _ => unreachable!(),
    }

    let color = CssColor::parse_string("#f0f").unwrap();
    assert_eq!(color.to_css_string(PrinterOptions::default()).unwrap(), "#f0f");

    let rule = CssRule::parse_string(".foo { color: red }", ParserOptions::default()).unwrap();
    assert_eq!(
      rule.to_css_string(PrinterOptions::default()).unwrap(),
      indoc! {r#"
    .foo {
      color: red;
    }"#}
    );

    let property = Property::parse_string("color".into(), "#f0f", ParserOptions::default()).unwrap();
    assert_eq!(
      property.to_css_string(false, PrinterOptions::default()).unwrap(),
      "color: #f0f"
    );
    assert_eq!(
      property.to_css_string(true, PrinterOptions::default()).unwrap(),
      "color: #f0f !important"
    );

    let code = indoc! { r#"
      .foo {
        color: green;
      }

      .bar {
        color: red;
        background: pink;
      }

      @media print {
        .baz {
          color: green;
        }
      }
    "#};
    let stylesheet = StyleSheet::parse(code, ParserOptions::default()).unwrap();
    if let CssRule::Style(style) = &stylesheet.rules.0[1] {
      let (key, val) = style.property_location(code, 0).unwrap();
      assert_eq!(
        key,
        SourceLocation { line: 5, column: 3 }..SourceLocation { line: 5, column: 8 }
      );
      assert_eq!(
        val,
        SourceLocation { line: 5, column: 10 }..SourceLocation { line: 5, column: 13 }
      );
    }

    if let CssRule::Style(style) = &stylesheet.rules.0[1] {
      let (key, val) = style.property_location(code, 1).unwrap();
      assert_eq!(
        key,
        SourceLocation { line: 6, column: 3 }..SourceLocation { line: 6, column: 13 }
      );
      assert_eq!(
        val,
        SourceLocation { line: 6, column: 15 }..SourceLocation { line: 6, column: 19 }
      );
    }
    if let CssRule::Media(media) = &stylesheet.rules.0[2] {
      if let CssRule::Style(style) = &media.rules.0[0] {
        let (key, val) = style.property_location(code, 0).unwrap();
        assert_eq!(
          key,
          SourceLocation { line: 11, column: 5 }..SourceLocation { line: 11, column: 10 }
        );
        assert_eq!(
          val,
          SourceLocation { line: 11, column: 12 }..SourceLocation { line: 11, column: 17 }
        );
      }
    }

    let mut property = Property::Transform(Default::default(), VendorPrefix::WebKit);
    property.set_prefix(VendorPrefix::None);
    assert_eq!(property, Property::Transform(Default::default(), VendorPrefix::None));
    property.set_prefix(VendorPrefix::Moz);
    assert_eq!(property, Property::Transform(Default::default(), VendorPrefix::Moz));
    property.set_prefix(VendorPrefix::WebKit | VendorPrefix::Moz);
    assert_eq!(
      property,
      Property::Transform(Default::default(), VendorPrefix::WebKit | VendorPrefix::Moz)
    );

    let mut property = Property::TextDecorationLine(Default::default(), VendorPrefix::None);
    property.set_prefix(VendorPrefix::Ms);
    assert_eq!(
      property,
      Property::TextDecorationLine(Default::default(), VendorPrefix::None)
    );
    property.set_prefix(VendorPrefix::WebKit | VendorPrefix::Moz | VendorPrefix::Ms);
    assert_eq!(
      property,
      Property::TextDecorationLine(Default::default(), VendorPrefix::WebKit | VendorPrefix::Moz)
    );

    let mut property = Property::AccentColor(Default::default());
    property.set_prefix(VendorPrefix::WebKit);
    assert_eq!(property, Property::AccentColor(Default::default()));
  }

  #[cfg(feature = "substitute_variables")]
  #[test]
  fn test_substitute_vars() {
    use crate::properties::custom::TokenList;
    use crate::traits::ParseWithOptions;

    fn test(property: Property, vars: HashMap<&str, &str>, expected: &str) {
      if let Property::Unparsed(unparsed) = property {
        let vars = vars
          .into_iter()
          .map(|(k, v)| {
            (
              k,
              TokenList::parse_string_with_options(v, ParserOptions::default()).unwrap(),
            )
          })
          .collect();
        let substituted = unparsed.substitute_variables(&vars).unwrap();
        assert_eq!(
          substituted.to_css_string(false, PrinterOptions::default()).unwrap(),
          expected
        );
      } else {
        panic!("Not an unparsed property");
      }
    }

    let property = Property::parse_string("color".into(), "var(--test)", ParserOptions::default()).unwrap();
    test(property, HashMap::from([("--test", "yellow")]), "color: #ff0");

    let property =
      Property::parse_string("color".into(), "var(--test, var(--foo))", ParserOptions::default()).unwrap();
    test(property, HashMap::from([("--foo", "yellow")]), "color: #ff0");
    let property = Property::parse_string(
      "color".into(),
      "var(--test, var(--foo, yellow))",
      ParserOptions::default(),
    )
    .unwrap();
    test(property, HashMap::new(), "color: #ff0");

    let property =
      Property::parse_string("width".into(), "calc(var(--a) + var(--b))", ParserOptions::default()).unwrap();
    test(property, HashMap::from([("--a", "2px"), ("--b", "4px")]), "width: 6px");

    let property = Property::parse_string("color".into(), "var(--a)", ParserOptions::default()).unwrap();
    test(
      property,
      HashMap::from([("--a", "var(--b)"), ("--b", "yellow")]),
      "color: #ff0",
    );

    let property = Property::parse_string("color".into(), "var(--a)", ParserOptions::default()).unwrap();
    test(
      property,
      HashMap::from([("--a", "var(--b)"), ("--b", "var(--c)"), ("--c", "var(--a)")]),
      "color: var(--a)",
    );
  }

  #[test]
  fn test_layer() {
    minify_test("@layer foo;", "@layer foo;");
    minify_test("@layer foo, bar;", "@layer foo,bar;");
    minify_test("@layer foo.bar;", "@layer foo.bar;");
    minify_test("@layer foo.bar, baz;", "@layer foo.bar,baz;");

    minify_test(
      r#"
      @layer foo {
        .bar {
          color: red;
        }
      }
    "#,
      "@layer foo{.bar{color:red}}",
    );
    minify_test(
      r#"
      @layer foo.bar {
        .bar {
          color: red;
        }
      }
    "#,
      "@layer foo.bar{.bar{color:red}}",
    );
    minify_test(r#"
      @layer base {
        p { max-width: 70ch; }
      }

      @layer framework {
        @layer base {
          p { margin-block: 0.75em; }
        }

        @layer theme {
          p { color: #222; }
        }
      }
    "#, "@layer base{p{max-width:70ch}}@layer framework{@layer base{p{margin-block:.75em}}@layer theme{p{color:#222}}}");
    minify_test(
      r#"
      @layer {
        .bar {
          color: red;
        }
      }
    "#,
      "@layer{.bar{color:red}}",
    );
    minify_test(
      r#"
      @layer foo\20 bar, baz;
    "#,
      "@layer foo\\ bar,baz;",
    );
    minify_test(
      r#"
      @layer one.two\20 three\#four\.five {
        .bar {
          color: red;
        }
      }
    "#,
      "@layer one.two\\ three\\#four\\.five{.bar{color:red}}",
    );

    error_test("@layer;", ParserError::UnexpectedToken(Token::Semicolon));
    error_test("@layer foo, bar {};", ParserError::AtRuleBodyInvalid);
    minify_test("@import 'test.css' layer;", "@import \"test.css\" layer;");
    minify_test("@import 'test.css' layer(foo);", "@import \"test.css\" layer(foo);");
    minify_test(
      "@import 'test.css' layer(foo.bar);",
      "@import \"test.css\" layer(foo.bar);",
    );
    minify_test(
      "@import 'test.css' layer(foo\\20 bar);",
      "@import \"test.css\" layer(foo\\ bar);",
    );
    error_test(
      "@import 'test.css' layer(foo, bar) {};",
      ParserError::UnexpectedToken(Token::Comma),
    );
    minify_test(
      r#"
      @layer one {
        body {
          background: red;
        }
      }

      body {
        background: red;
      }

      @layer two {
        body {
          background: green;
        }
      }

      @layer one {
        body {
          background: yellow;
        }
      }
      "#,
      "@layer one{body{background:#ff0}}body{background:red}@layer two{body{background:green}}",
    );
  }

  #[test]
  fn test_property() {
    minify_test(
      r#"
      @property --property-name {
        syntax: '<color>';
        inherits: false;
        initial-value: yellow;
      }
    "#,
      "@property --property-name{syntax:\"<color>\";inherits:false;initial-value:#ff0}",
    );

    test(
      r#"
      @property --property-name {
        syntax: '*';
        inherits: false;
        initial-value: ;
      }
    "#,
      indoc! {r#"
      @property --property-name {
        syntax: "*";
        inherits: false;
        initial-value: ;
      }
    "#},
    );

    minify_test(
      r#"
      @property --property-name {
        syntax: '*';
        inherits: false;
        initial-value: ;
      }
    "#,
      "@property --property-name{syntax:\"*\";inherits:false;initial-value:}",
    );

    test(
      r#"
      @property --property-name {
        syntax: '*';
        inherits: false;
        initial-value:;
      }
    "#,
      indoc! {r#"
      @property --property-name {
        syntax: "*";
        inherits: false;
        initial-value: ;
      }
    "#},
    );

    minify_test(
      r#"
      @property --property-name {
        syntax: '*';
        inherits: false;
        initial-value:;
      }
    "#,
      "@property --property-name{syntax:\"*\";inherits:false;initial-value:}",
    );
    minify_test(
      r#"
      @property --property-name {
        syntax: '*';
        inherits: false;
        initial-value: foo bar;
      }
    "#,
      "@property --property-name{syntax:\"*\";inherits:false;initial-value:foo bar}",
    );

    minify_test(
      r#"
      @property --property-name {
        syntax: '<length>';
        inherits: true;
        initial-value: 25px;
      }
    "#,
      "@property --property-name{syntax:\"<length>\";inherits:true;initial-value:25px}",
    );

    error_test(
      r#"
      @property --property-name {
        syntax: '<color>';
        inherits: false;
        initial-value: 25px;
      }
    "#,
      ParserError::UnexpectedToken(crate::properties::custom::Token::Dimension {
        has_sign: false,
        value: 25.0,
        int_value: Some(25),
        unit: "px".into(),
      }),
    );

    error_test(
      r#"
      @property --property-name {
        syntax: '<length>';
        inherits: false;
        initial-value: var(--some-value);
      }
    "#,
      ParserError::UnexpectedToken(crate::properties::custom::Token::Function("var".into())),
    );

    error_test(
      r#"
      @property --property-name {
        syntax: '<color>';
        inherits: false;
      }
    "#,
      ParserError::AtRuleBodyInvalid,
    );

    minify_test(
      r#"
      @property --property-name {
        syntax: '*';
        inherits: false;
      }
    "#,
      "@property --property-name{syntax:\"*\";inherits:false}",
    );

    error_test(
      r#"
      @property --property-name {
        syntax: '*';
      }
    "#,
      ParserError::AtRuleBodyInvalid,
    );

    error_test(
      r#"
      @property --property-name {
        inherits: false;
      }
    "#,
      ParserError::AtRuleBodyInvalid,
    );

    error_test(
      r#"
      @property property-name {
        syntax: '*';
        inherits: false;
      }
    "#,
      ParserError::UnexpectedToken(crate::properties::custom::Token::Ident("property-name".into())),
    );

    minify_test(
      r#"
      @property --property-name {
        syntax: 'custom | <color>';
        inherits: false;
        initial-value: yellow;
      }
    "#,
      "@property --property-name{syntax:\"custom|<color>\";inherits:false;initial-value:#ff0}",
    );

    // TODO: Re-enable with a better solution
    //       See: https://github.com/parcel-bundler/lightningcss/issues/288
    // minify_test(r#"
    //   @property --property-name {
    //     syntax: '<transform-list>';
    //     inherits: false;
    //     initial-value: translate(200px,300px) translate(100px,200px) scale(2);
    //   }
    // "#, "@property --property-name{syntax:\"<transform-list>\";inherits:false;initial-value:matrix(2,0,0,2,300,500)}");

    minify_test(
      r#"
      @property --property-name {
        syntax: '<time>';
        inherits: false;
        initial-value: 1000ms;
      }
    "#,
      "@property --property-name{syntax:\"<time>\";inherits:false;initial-value:1s}",
    );

    minify_test(
      r#"
      @property --property-name {
        syntax: '<url>';
        inherits: false;
        initial-value: url("foo.png");
      }
    "#,
      "@property --property-name{syntax:\"<url>\";inherits:false;initial-value:url(foo.png)}",
    );

    minify_test(
      r#"
      @property --property-name {
        syntax: '<image>';
        inherits: false;
        initial-value: linear-gradient(yellow, blue);
      }
    "#,
      "@property --property-name{syntax:\"<image>\";inherits:false;initial-value:linear-gradient(#ff0,#00f)}",
    );

    minify_test(
      r#"
      @property --property-name {
        initial-value: linear-gradient(yellow, blue);
        inherits: false;
        syntax: '<image>';
      }
    "#,
      "@property --property-name{syntax:\"<image>\";inherits:false;initial-value:linear-gradient(#ff0,#00f)}",
    );

    test(
      r#"
      @property --property-name {
        syntax: '<length>|none';
        inherits: false;
        initial-value: none;
      }
    "#,
      indoc! {r#"
      @property --property-name {
        syntax: "<length> | none";
        inherits: false;
        initial-value: none;
      }
    "#},
    );

    minify_test(
      r#"
      @property --property-name {
        syntax: '<color>#';
        inherits: false;
        initial-value: yellow, blue;
      }
    "#,
      "@property --property-name{syntax:\"<color>#\";inherits:false;initial-value:#ff0,#00f}",
    );
    minify_test(
      r#"
      @property --property-name {
        syntax: '<color>+';
        inherits: false;
        initial-value: yellow blue;
      }
    "#,
      "@property --property-name{syntax:\"<color>+\";inherits:false;initial-value:#ff0 #00f}",
    );
    minify_test(
      r#"
      @property --property-name {
        syntax: '<color>';
        inherits: false;
        initial-value: yellow;
      }
      .foo {
        color: var(--property-name)
      }
      @property --property-name {
        syntax: '<color>';
        inherits: true;
        initial-value: blue;
      }
    "#,
      "@property --property-name{syntax:\"<color>\";inherits:true;initial-value:#00f}.foo{color:var(--property-name)}",
    );
  }

  #[test]
  fn test_quoting_unquoting_urls() {
    // Quotes remain double quotes when not minifying
    test(
      r#".foo {
      background-image: url("0123abcd");
    }"#,
      r#".foo {
  background-image: url("0123abcd");
}
"#,
    );

    // Quotes removed when minifying
    minify_test(
      r#".foo {
      background-image: url("0123abcd");
    }"#,
      r#".foo{background-image:url(0123abcd)}"#,
    );

    // Doubles quotes added if not present when not minifying
    test(
      r#".foo {
      background-image: url(0123abcd);
    }"#,
      r#".foo {
  background-image: url("0123abcd");
}
"#,
    );

    // No quotes changed if not present when not minifying
    minify_test(
      r#".foo {
      background-image: url(0123abcd);
    }"#,
      r#".foo{background-image:url(0123abcd)}"#,
    );
  }

  #[test]
  fn test_zindex() {
    minify_test(".foo { z-index: 2 }", ".foo{z-index:2}");
    minify_test(".foo { z-index: -2 }", ".foo{z-index:-2}");
    minify_test(".foo { z-index: 999999 }", ".foo{z-index:999999}");
    minify_test(".foo { z-index: 9999999 }", ".foo{z-index:9999999}");
    minify_test(".foo { z-index: -9999999 }", ".foo{z-index:-9999999}");
  }

  #[test]
  #[cfg(feature = "sourcemap")]
  fn test_input_source_map() {
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

    let mut stylesheet = StyleSheet::parse(&source, ParserOptions::default()).unwrap();
    stylesheet.minify(MinifyOptions::default()).unwrap();
    let mut sm = parcel_sourcemap::SourceMap::new("/");
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
      r#"{"version":3,"sourceRoot":null,"mappings":"AAAA,uCCGA,2CAAA","sources":["sass/_demo.scss","stdin"],"sourcesContent":[".imported {\n  content: \"yay, file support!\";\n}","@import \"_variables\";\n@import \"_demo\";\n\n.selector {\n  margin: $size;\n  background-color: $brandColor;\n\n  .nested {\n    margin: $size / 2;\n  }\n}"],"names":[]}"#
    );
  }

  #[test]
  fn test_error_recovery() {
    use std::sync::{Arc, RwLock};
    let warnings = Some(Arc::new(RwLock::new(Vec::new())));
    test_with_options(
      r#"
      h1(>h1) {
        color: red;
      }

      .foo {
        color: red;
      }

      .clearfix {
        *zoom: 1;
        background: red;
      }

      @media (hover) {
        h1(>h1) {
          color: red;
        }

        .bar {
          color: red;
        }
      }

      input:placeholder {
        color: red;
      }

      input::hover {
        color: red;
      }
    "#,
      indoc! { r#"
      .foo {
        color: red;
      }

      .clearfix {
        background: red;
      }

      @media (hover) {
        .bar {
          color: red;
        }
      }

      input:placeholder {
        color: red;
      }

      input::hover {
        color: red;
      }
    "#},
      ParserOptions {
        filename: "test.css".into(),
        error_recovery: true,
        warnings: warnings.clone(),
        ..ParserOptions::default()
      },
    );
    let w = warnings.unwrap();
    let warnings = w.read().unwrap();
    assert_eq!(
      *warnings,
      vec![
        Error {
          kind: ParserError::SelectorError(SelectorError::EmptySelector),
          loc: Some(ErrorLocation {
            filename: "test.css".into(),
            line: 1,
            column: 7
          })
        },
        Error {
          kind: ParserError::UnexpectedToken(Token::Semicolon),
          loc: Some(ErrorLocation {
            filename: "test.css".into(),
            line: 10,
            column: 17
          })
        },
        Error {
          kind: ParserError::SelectorError(SelectorError::EmptySelector),
          loc: Some(ErrorLocation {
            filename: "test.css".into(),
            line: 15,
            column: 9
          })
        },
        Error {
          kind: ParserError::SelectorError(SelectorError::UnsupportedPseudoClass("placeholder".into())),
          loc: Some(ErrorLocation {
            filename: "test.css".into(),
            line: 24,
            column: 13,
          }),
        },
        Error {
          kind: ParserError::SelectorError(SelectorError::UnsupportedPseudoElement("hover".into())),
          loc: Some(ErrorLocation {
            filename: "test.css".into(),
            line: 28,
            column: 13,
          }),
        },
      ]
    )
  }

  #[test]
  fn test_invalid() {
    error_test(
      ".a{color: hsla(120, 62.32%;}",
      ParserError::UnexpectedToken(Token::CloseCurlyBracket),
    );
    error_test(
      ".a{--foo: url(foo\\) b\\)ar)}",
      ParserError::UnexpectedToken(Token::BadUrl("foo\\) b\\)ar".into())),
    );
  }

  #[test]
  fn test_container_queries() {
    // with name
    minify_test(
      r#"
      @container my-layout (inline-size > 45em) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container my-layout (inline-size>45em){.foo{color:red}}",
    );

    minify_test(
      r#"
      @container my-layout ( not (width > 500px) ) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container my-layout not (width>500px){.foo{color:red}}",
    );

    minify_test(
      r#"
      @container my-layout not (width > 500px) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container my-layout not (width>500px){.foo{color:red}}",
    );

    minify_test(
      r#"
      @container not (width > 500px) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container not (width>500px){.foo{color:red}}",
    );

    minify_test(
      r#"
      @container my-layout ((width: 100px) and (not (height: 100px))) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container my-layout (width:100px) and (not (height:100px)){.foo{color:red}}",
    );

    minify_test(
      r#"
      @container my-layout (width = max(10px, 10em)) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container my-layout (width=max(10px,10em)){.foo{color:red}}",
    );

    // without name
    minify_test(
      r#"
      @container (inline-size > 45em) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container (inline-size>45em){.foo{color:red}}",
    );

    minify_test(
      r#"
      @container (inline-size > 45em) and (inline-size < 100em) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container (inline-size>45em) and (inline-size<100em){.foo{color:red}}",
    );

    // calc()
    minify_test(
      r#"
      @container (width > calc(100vw - 50px)) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container (width>calc(100vw - 50px)){.foo{color:red}}",
    );

    minify_test(
      r#"
      @container (calc(100vh - 50px) <= height ) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container (height>=calc(100vh - 50px)){.foo{color:red}}",
    );

    // merge adjacent
    minify_test(
      r#"
      @container my-layout (inline-size > 45em) {
        .foo {
          color: red;
        }
      }

      @container my-layout (inline-size > 45em) {
        .foo {
          background: yellow;
        }

        .bar {
          color: white;
        }
      }
    "#,
      "@container my-layout (inline-size>45em){.foo{color:red;background:#ff0}.bar{color:#fff}}",
    );

    minify_test(
      r#"
    .foo {
      container-name: foo bar;
      container-type: size;
    }
    "#,
      ".foo{container:foo bar/size}",
    );
    minify_test(
      r#"
    .foo {
      container-name: foo bar;
      container-type: normal;
    }
    "#,
      ".foo{container:foo bar}",
    );
    minify_test(
      ".foo{ container-type: inline-size }",
      ".foo{container-type:inline-size}",
    );
    minify_test(".foo{ container-name: none; }", ".foo{container-name:none}");
    minify_test(".foo{ container-name: foo; }", ".foo{container-name:foo}");
    minify_test(".foo{ container: foo / normal; }", ".foo{container:foo}");
    minify_test(
      ".foo{ container: foo / inline-size; }",
      ".foo{container:foo/inline-size}",
    );
    minify_test(".foo { width: calc(1cqw + 2cqw) }", ".foo{width:3cqw}");
    minify_test(".foo { width: calc(1cqh + 2cqh) }", ".foo{width:3cqh}");
    minify_test(".foo { width: calc(1cqi + 2cqi) }", ".foo{width:3cqi}");
    minify_test(".foo { width: calc(1cqb + 2cqb) }", ".foo{width:3cqb}");
    minify_test(".foo { width: calc(1cqmin + 2cqmin) }", ".foo{width:3cqmin}");
    minify_test(".foo { width: calc(1cqmax + 2cqmax) }", ".foo{width:3cqmax}");

    // Unlike in @media, there is no need to convert the range syntax in @container,
    // because browsers all support this syntax.
    prefix_test(
      r#"
      @container (width > 100px) {
        .foo { padding: 5px; }
      }
      "#,
      indoc! { r#"
        @container (width > 100px) {
          .foo {
            padding: 5px;
          }
        }
      "#},
      Browsers {
        chrome: Some(105 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      @container (min-width: 100px) {
        .foo { padding: 5px; }
      }
      "#,
      indoc! { r#"
        @container (width >= 100px) {
          .foo {
            padding: 5px;
          }
        }
      "#},
      Browsers {
        chrome: Some(105 << 16),
        ..Browsers::default()
      },
    );

    minify_test(
      r#"
      @container style(--responsive: true) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container style(--responsive:true){.foo{color:red}}",
    );
    minify_test(
      r#"
      @container style(--responsive: true) and style(color: yellow) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container style(--responsive:true) and style(color:#ff0){.foo{color:red}}",
    );
    minify_test(
      r#"
      @container not style(--responsive: true) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container not style(--responsive:true){.foo{color:red}}",
    );
    minify_test(
      r#"
      @container (inline-size > 45em) and style(--responsive: true) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container (inline-size>45em) and style(--responsive:true){.foo{color:red}}",
    );
    minify_test(
      r#"
      @container style((accent-color: yellow) or (--bar: 10px)) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container style((accent-color:#ff0) or (--bar:10px)){.foo{color:red}}",
    );
    minify_test(
      r#"
      @container style(not ((width: calc(10px + 20px)) and ((--bar: url(x))))) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container style(not ((width:30px) and (--bar:url(x)))){.foo{color:red}}",
    );
    minify_test(
      r#"
      @container style(color: yellow !important) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container style(color:yellow){.foo{color:red}}",
    );
    minify_test(
      r#"
      @container style(--foo:) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container style(--foo:){.foo{color:red}}",
    );
    minify_test(
      r#"
      @container style(--foo: ) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container style(--foo:){.foo{color:red}}",
    );
    minify_test(
      r#"
      @container style(--my-prop: foo - bar ()) {
        .foo {
          color: red;
        }
      }
    "#,
      "@container style(--my-prop:foo - bar ()){.foo{color:red}}",
    );

    // Disallow 'none', 'not', 'and', 'or' as a `<container-name>`
    // https://github.com/w3c/csswg-drafts/issues/7203#issuecomment-1144257312
    // https://chromium-review.googlesource.com/c/chromium/src/+/3698402
    error_test(
      "@container none (width < 100vw) {}",
      ParserError::UnexpectedToken(crate::properties::custom::Token::Ident("none".into())),
    );

    error_test(
      "@container and (width < 100vw) {}",
      ParserError::UnexpectedToken(crate::properties::custom::Token::Ident("and".into())),
    );

    error_test(
      "@container or (width < 100vw) {}",
      ParserError::UnexpectedToken(crate::properties::custom::Token::Ident("or".into())),
    );

    // Disallow CSS wide keywords as a `<container-name>`
    error_test(
      "@container revert-layer (width < 100vw) {}",
      ParserError::UnexpectedToken(crate::properties::custom::Token::Ident("revert-layer".into())),
    );

    error_test(
      "@container initial (width < 100vw) {}",
      ParserError::UnexpectedToken(crate::properties::custom::Token::Ident("initial".into())),
    );

    // <ident> contains spaces
    // https://github.com/web-platform-tests/wpt/blob/39f0da08fbbe33d0582a35749b6dbf8bd067a52d/css/css-contain/container-queries/at-container-parsing.html#L160-L178
    error_test(
      "@container foo bar (width < 100vw) {}",
      ParserError::UnexpectedToken(crate::properties::custom::Token::Ident("bar".into())),
    );

    error_test("@container (inline-size <= foo) {}", ParserError::InvalidMediaQuery);
    error_test("@container (orientation <= 10px) {}", ParserError::InvalidMediaQuery);

    error_test("@container style(width) {}", ParserError::EndOfInput);
    error_test(
      "@container style(style(--foo: bar)) {}",
      ParserError::UnexpectedToken(crate::properties::custom::Token::Function("style".into())),
    );
  }

  #[test]
  fn test_css_modules_value_rule() {
    css_modules_error_test(
      "@value compact: (max-width: 37.4375em);",
      ParserError::DeprecatedCssModulesValueRule,
    );
  }

  #[test]
  fn test_unknown_at_rules() {
    minify_test("@foo;", "@foo;");
    minify_test("@foo bar;", "@foo bar;");
    minify_test("@foo (bar: baz);", "@foo (bar: baz);");
    test(
      r#"@foo test {
      div {
        color: red;
      }
    }"#,
      indoc! {r#"
      @foo test {
        div { color: red; }
      }
      "#},
    );
    minify_test(
      r#"@foo test {
      div {
        color: red;
      }
    }"#,
      "@foo test{div { color: red; }}",
    );
    minify_test(
      r#"@foo test {
        foo: bar;
      }"#,
      "@foo test{foo: bar;}",
    );
    test(
      r#"@foo {
        foo: bar;
      }"#,
      indoc! {r#"
      @foo {
        foo: bar;
      }
      "#},
    );
    minify_test(
      r#"@foo {
        foo: bar;
      }"#,
      "@foo{foo: bar;}",
    );
  }

  #[test]
  fn test_resolution() {
    prefix_test(
      r#"
      @media (resolution: 1dppx) {
        body {
          background: red;
        }
      }
      "#,
      indoc! { r#"
      @media (resolution: 1dppx) {
        body {
          background: red;
        }
      }
      "#},
      Browsers {
        chrome: Some(50 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      r#"
      @media (resolution: 1dppx) {
        body {
          background: red;
        }
      }
      "#,
      indoc! { r#"
      @media (resolution: 1x) {
        body {
          background: red;
        }
      }
      "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );
  }

  #[test]
  fn test_environment() {
    minify_test(
      r#"
      @media (max-width: env(--branding-small)) {
        body {
          padding: env(--branding-padding);
        }
      }
    "#,
      "@media (width<=env(--branding-small)){body{padding:env(--branding-padding)}}",
    );

    minify_test(
      r#"
      @media (max-width: env(--branding-small 1)) {
        body {
          padding: env(--branding-padding 2);
        }
      }
    "#,
      "@media (width<=env(--branding-small 1)){body{padding:env(--branding-padding 2)}}",
    );

    minify_test(
      r#"
      @media (max-width: env(--branding-small 1, 20px)) {
        body {
          padding: env(--branding-padding 2, 20px);
        }
      }
    "#,
      "@media (width<=env(--branding-small 1,20px)){body{padding:env(--branding-padding 2,20px)}}",
    );

    minify_test(
      r#"
      @media (max-width: env(safe-area-inset-top)) {
        body {
          padding: env(safe-area-inset-top);
        }
      }
    "#,
      "@media (width<=env(safe-area-inset-top)){body{padding:env(safe-area-inset-top)}}",
    );

    minify_test(
      r#"
      @media (max-width: env(unknown)) {
        body {
          padding: env(unknown);
        }
      }
    "#,
      "@media (width<=env(unknown)){body{padding:env(unknown)}}",
    );

    prefix_test(
      r#"
      .foo {
        color: env(--brand-color, color(display-p3 0 1 0));
      }
    "#,
      indoc! {r#"
      .foo {
        color: env(--brand-color, #00f942);
      }

      @supports (color: color(display-p3 0 0 0)) {
        .foo {
          color: env(--brand-color, color(display-p3 0 1 0));
        }
      }
    "#},
      Browsers {
        safari: Some(15 << 16),
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );

    css_modules_test(
      r#"
      @media (max-width: env(--branding-small)) {
        .foo {
          color: env(--brand-color);
        }
      }
    "#,
      indoc! {r#"
      @media (width <= env(--EgL3uq_branding-small)) {
        .EgL3uq_foo {
          color: env(--EgL3uq_brand-color);
        }
      }
    "#},
      map! {
        "foo" => "EgL3uq_foo",
        "--brand-color" => "--EgL3uq_brand-color" referenced: true,
        "--branding-small" => "--EgL3uq_branding-small" referenced: true
      },
      HashMap::new(),
      crate::css_modules::Config {
        dashed_idents: true,
        ..Default::default()
      },
      false,
    );
  }

  #[test]
  fn test_license_comments() {
    minify_test(
      r#"
      /*! Copyright 2023 Someone awesome */
      /* Some other comment */
      .foo {
        color: red;
      }
    "#,
      indoc! {r#"
      /*! Copyright 2023 Someone awesome */
      .foo{color:red}"#},
    );

    minify_test(
      r#"
      /*! Copyright 2023 Someone awesome */
      /*! Copyright 2023 Someone else */
      .foo {
        color: red;
      }
    "#,
      indoc! {r#"
      /*! Copyright 2023 Someone awesome */
      /*! Copyright 2023 Someone else */
      .foo{color:red}"#},
    );
  }

  #[test]
  fn test_starting_style() {
    minify_test(
      r#"
      @starting-style {
        h1 {
          background: yellow;
        }
      }
      "#,
      "@starting-style{h1{background:#ff0}}",
    );
    minify_test("@starting-style {}", "");

    nesting_test(
      r#"
      h1 {
        background: red;
        @starting-style {
          background: yellow;
        }
      }
      "#,
      indoc! {r#"
      h1 {
        background: red;
      }

      @starting-style {
        h1 {
          background: #ff0;
        }
      }
      "#},
    );
  }

  #[test]
  fn test_color_scheme() {
    minify_test(".foo { color-scheme: normal; }", ".foo{color-scheme:normal}");
    minify_test(".foo { color-scheme: light; }", ".foo{color-scheme:light}");
    minify_test(".foo { color-scheme: dark; }", ".foo{color-scheme:dark}");
    minify_test(".foo { color-scheme: light dark; }", ".foo{color-scheme:light dark}");
    minify_test(".foo { color-scheme: dark light; }", ".foo{color-scheme:light dark}");
    minify_test(".foo { color-scheme: only light; }", ".foo{color-scheme:light only}");
    minify_test(".foo { color-scheme: only dark; }", ".foo{color-scheme:dark only}");
    minify_test(
      ".foo { color-scheme: dark light only; }",
      ".foo{color-scheme:light dark only}",
    );
    minify_test(".foo { color-scheme: foo bar light; }", ".foo{color-scheme:light}");
    minify_test(
      ".foo { color-scheme: only foo dark bar; }",
      ".foo{color-scheme:dark only}",
    );
    prefix_test(
      ".foo { color-scheme: dark; }",
      indoc! { r#"
      .foo {
        --lightningcss-light: ;
        --lightningcss-dark: initial;
        color-scheme: dark;
      }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo { color-scheme: light; }",
      indoc! { r#"
      .foo {
        --lightningcss-light: initial;
        --lightningcss-dark: ;
        color-scheme: light;
      }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo { color-scheme: light dark; }",
      indoc! { r#"
      .foo {
        --lightningcss-light: initial;
        --lightningcss-dark: ;
        color-scheme: light dark;
      }

      @media (prefers-color-scheme: dark) {
        .foo {
          --lightningcss-light: ;
          --lightningcss-dark: initial;
        }
      }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo { color-scheme: light dark; }",
      indoc! { r#"
      .foo {
        color-scheme: light dark;
      }
      "#},
      Browsers {
        firefox: Some(120 << 16),
        ..Browsers::default()
      },
    );

    minify_test(
      ".foo { color: light-dark(yellow, red); }",
      ".foo{color:light-dark(#ff0,red)}",
    );
    minify_test(
      ".foo { color: light-dark(light-dark(yellow, red), light-dark(yellow, red)); }",
      ".foo{color:light-dark(#ff0,red)}",
    );
    minify_test(
      ".foo { color: light-dark(rgb(0, 0, 255), hsl(120deg, 50%, 50%)); }",
      ".foo{color:light-dark(#00f,#40bf40)}",
    );
    prefix_test(
      ".foo { color: light-dark(oklch(40% 0.1268735435 34.568626), oklab(59.686% 0.1009 0.1192)); }",
      indoc! { r#"
      .foo {
        color: var(--lightningcss-light, #7e250f) var(--lightningcss-dark, #c65d07);
        color: var(--lightningcss-light, lab(29.2661% 38.2437 35.3889)) var(--lightningcss-dark, lab(52.2319% 40.1449 59.9171));
      }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo { color: light-dark(oklch(40% 0.1268735435 34.568626), oklab(59.686% 0.1009 0.1192)); }",
      indoc! { r#"
      .foo {
        color: light-dark(oklch(40% .126874 34.5686), oklab(59.686% .1009 .1192));
      }
      "#},
      Browsers {
        firefox: Some(120 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        box-shadow:
            oklch(100% 0 0deg / 50%) 0 0.63rem 0.94rem -0.19rem,
            currentColor 0 0.44rem 0.8rem -0.58rem;
      }
    "#,
      indoc! { r#"
      .foo {
        box-shadow: 0 .63rem .94rem -.19rem #ffffff80, 0 .44rem .8rem -.58rem;
        box-shadow: 0 .63rem .94rem -.19rem lab(100% 0 0 / .5), 0 .44rem .8rem -.58rem;
      }
      "#},
      Browsers {
        chrome: Some(95 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      r#"
      .foo {
        box-shadow:
            oklch(100% 0 0deg / 50%) 0 0.63rem 0.94rem -0.19rem,
            currentColor 0 0.44rem 0.8rem -0.58rem;
      }
    "#,
      indoc! { r#"
      .foo {
        box-shadow: 0 .63rem .94rem -.19rem color(display-p3 1 1 1 / .5), 0 .44rem .8rem -.58rem;
        box-shadow: 0 .63rem .94rem -.19rem lab(100% 0 0 / .5), 0 .44rem .8rem -.58rem;
      }
      "#},
      Browsers {
        safari: Some(14 << 16),
        ..Browsers::default()
      },
    );

    prefix_test(
      ".foo { color: light-dark(var(--light), var(--dark)); }",
      indoc! { r#"
      .foo {
        color: var(--lightningcss-light, var(--light)) var(--lightningcss-dark, var(--dark));
      }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo { color: rgb(from light-dark(yellow, red) r g b / 10%); }",
      indoc! { r#"
      .foo {
        color: var(--lightningcss-light, #ffff001a) var(--lightningcss-dark, #ff00001a);
      }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo { color: rgb(from light-dark(yellow, red) r g b / var(--alpha)); }",
      indoc! { r#"
      .foo {
        color: var(--lightningcss-light, rgb(255 255 0 / var(--alpha))) var(--lightningcss-dark, rgb(255 0 0 / var(--alpha)));
      }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo { color: color(from light-dark(yellow, red) srgb r g b / 10%); }",
      indoc! { r#"
      .foo {
        color: var(--lightningcss-light, #ffff001a) var(--lightningcss-dark, #ff00001a);
        color: var(--lightningcss-light, color(srgb 1 1 0 / .1)) var(--lightningcss-dark, color(srgb 1 0 0 / .1));
      }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    prefix_test(
      ".foo { color: color-mix(in srgb, light-dark(yellow, red), light-dark(red, pink)); }",
      indoc! { r#"
      .foo {
        color: var(--lightningcss-light, #ff8000) var(--lightningcss-dark, #ff6066);
      }
      "#},
      Browsers {
        chrome: Some(90 << 16),
        ..Browsers::default()
      },
    );
    nesting_test_with_targets(
      r#"
        .foo { color-scheme: light; }
        .bar { color: light-dark(red, green); }
      "#,
      indoc! {r#"
        .foo {
          color-scheme: light;
        }

        .bar {
          color: light-dark(red, green);
        }
      "#},
      Targets {
        browsers: Some(Browsers {
          safari: Some(13 << 16),
          ..Browsers::default()
        }),
        include: Features::empty(),
        exclude: Features::LightDark,
      },
    );
  }

  #[test]
  fn test_all() {
    minify_test(".foo { all: initial; all: initial }", ".foo{all:initial}");
    minify_test(".foo { all: initial; all: revert }", ".foo{all:revert}");
    minify_test(".foo { background: red; all: revert-layer }", ".foo{all:revert-layer}");
    minify_test(
      ".foo { background: red; all: revert-layer; background: green }",
      ".foo{all:revert-layer;background:green}",
    );
    minify_test(
      ".foo { --test: red; all: revert-layer }",
      ".foo{--test:red;all:revert-layer}",
    );
    minify_test(
      ".foo { unicode-bidi: embed; all: revert-layer }",
      ".foo{all:revert-layer;unicode-bidi:embed}",
    );
    minify_test(
      ".foo { direction: rtl; all: revert-layer }",
      ".foo{all:revert-layer;direction:rtl}",
    );
    minify_test(
      ".foo { direction: rtl; all: revert-layer; direction: ltr }",
      ".foo{all:revert-layer;direction:ltr}",
    );
    minify_test(".foo { background: var(--foo); all: unset; }", ".foo{all:unset}");
    minify_test(
      ".foo { all: unset; background: var(--foo); }",
      ".foo{all:unset;background:var(--foo)}",
    );
    minify_test(
      ".foo {--bar:currentcolor; --foo:1.1em; all:unset}",
      ".foo{--bar:currentcolor;--foo:1.1em;all:unset}",
    );
  }

  #[test]
  fn test_view_transition() {
    minify_test(
      "@view-transition { navigation: auto }",
      "@view-transition{navigation:auto}",
    );
    minify_test(
      "@view-transition { navigation: auto; types: none; }",
      "@view-transition{navigation:auto;types:none}",
    );
    minify_test(
      "@view-transition { navigation: auto; types: foo bar; }",
      "@view-transition{navigation:auto;types:foo bar}",
    );
    minify_test(
      "@layer { @view-transition { navigation: auto; types: foo bar; } }",
      "@layer{@view-transition{navigation:auto;types:foo bar}}",
    );
  }
}
