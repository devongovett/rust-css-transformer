//! CSS module exports.
//!
//! [CSS modules](https://github.com/css-modules/css-modules) are a way of locally scoping names in a
//! CSS file. This includes class names, ids, keyframe animation names, and any other places where the
//! [CustomIdent](super::values::ident::CustomIdent) type is used.
//!
//! CSS modules can be enabled using the `css_modules` option when parsing a style sheet. When the
//! style sheet is printed, hashes will be added to any declared names, and references to those names
//! will be updated accordingly. A map of the original names to compiled (hashed) names will be returned.

use crate::error::{PrinterError, PrinterErrorKind};
use crate::printer::Printer;
use crate::properties::css_modules::{Composes, ComposesFrom};
use crate::selector::Selectors;
use crate::traits::ToCss;
use cssparser::serialize_identifier;
use data_encoding::{Encoding, Specification};
use lazy_static::lazy_static;
use parcel_selectors::SelectorList;
use serde::Serialize;
use smallvec::{smallvec, SmallVec};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::fmt::Write;
use std::hash::{Hash, Hasher};
use std::path::Path;

/// Configuration for CSS modules.
#[derive(Default, Clone, Debug)]
pub struct Config<'i> {
  /// [dpfhjod[ofpihd]]
  pub pattern: Pattern<'i>,
}

/// A CSS modules class name pattern.
#[derive(Clone, Debug)]
pub struct Pattern<'i> {
  segments: SmallVec<[Segment<'i>; 2]>,
}

impl<'i> Default for Pattern<'i> {
  fn default() -> Self {
    Pattern {
      segments: smallvec![Segment::Hash, Segment::Literal("_"), Segment::Local],
    }
  }
}

impl<'i> Pattern<'i> {
  /// dopifhdoifhdofih
  pub fn parse(mut input: &'i str) -> Result<Self, ()> {
    let mut segments = SmallVec::new();
    while !input.is_empty() {
      if input.starts_with('[') {
        if let Some(end_idx) = input.find(']') {
          let segment = match &input[0..=end_idx] {
            "[name]" => Segment::Name,
            "[local]" => Segment::Local,
            "[hash]" => Segment::Hash,
            _ => return Err(()),
          };
          segments.push(segment);
          input = &input[end_idx + 1..];
        } else {
          return Err(());
        }
      } else {
        let end_idx = input.find('[').unwrap_or_else(|| input.len());
        segments.push(Segment::Literal(&input[0..end_idx]));
        input = &input[end_idx..];
      }
    }

    Ok(Pattern { segments })
  }

  /// dpofihdoifhd
  pub fn write<W, E>(&self, hash: &str, local: &str, mut write: W) -> Result<(), E>
  where
    W: FnMut(&str) -> Result<(), E>,
  {
    for segment in &self.segments {
      // segment.write(css_module, local, dest)?;
      match segment {
        Segment::Literal(s) => {
          write(s)?;
        }
        // Segment::Name => {
        //   let name = dest.filename();
        //   let path = Path::new(name);
        //   let basename = path.file_name().map(|name| name.split('.'));
        // }
        Segment::Local => {
          write(local)?;
        }
        Segment::Hash => {
          write(hash)?;
        }
        _ => todo!(),
      }
    }
    Ok(())
  }

  fn write_to_string(&self, hash: &str, local: &str) -> Result<String, std::fmt::Error> {
    let mut res = String::new();
    self.write(hash, local, |s| res.write_str(s))?;
    Ok(res)
  }
}

/// A segment in a CSS modules class name pattern.
///
/// See [Pattern](Pattern).
#[derive(Clone, Debug)]
pub enum Segment<'i> {
  /// A literal string segment.
  Literal(&'i str),
  /// The base file name.
  Name,
  /// The original class name.
  Local,
  /// A hash of the file name.
  Hash,
}

/// A referenced name within a CSS module, e.g. via the `composes` property.
///
/// See [CssModuleExport](CssModuleExport).
#[derive(PartialEq, Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CssModuleReference {
  /// A local reference.
  Local {
    /// The local (compiled) name for the reference.
    name: String,
  },
  /// A global reference.
  Global {
    /// The referenced global name.
    name: String,
  },
  /// A reference to an export in a different file.
  Dependency {
    /// The name to reference within the dependency.
    name: String,
    /// The dependency specifier for the referenced file.
    specifier: String,
  },
}

/// An exported value from a CSS module.
#[derive(PartialEq, Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CssModuleExport {
  /// The local (compiled) name for this export.
  pub name: String,
  /// Other names that are composed by this export.
  pub composes: Vec<CssModuleReference>,
  /// Whether the export is referenced in this file.
  pub is_referenced: bool,
}

/// A map of exported names to values.
pub type CssModuleExports = HashMap<String, CssModuleExport>;

lazy_static! {
  static ref ENCODER: Encoding = {
    let mut spec = Specification::new();
    spec
      .symbols
      .push_str("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890_-");
    spec.encoding().unwrap()
  };
}

pub(crate) struct CssModule<'a, 'b> {
  pub config: &'a Config<'b>,
  pub hash: String,
  pub exports: &'a mut CssModuleExports,
}

impl<'a, 'b> CssModule<'a, 'b> {
  pub fn add_local(&mut self, exported: &str, local: &str) {
    let hash = &self.hash;
    self.exports.entry(exported.into()).or_insert_with(|| CssModuleExport {
      name: self.config.pattern.write_to_string(hash, local).unwrap(),
      composes: vec![],
      is_referenced: false,
    });
  }

  pub fn reference(&mut self, name: &str) {
    let hash = &self.hash;
    match self.exports.entry(name.into()) {
      std::collections::hash_map::Entry::Occupied(mut entry) => {
        entry.get_mut().is_referenced = true;
      }
      std::collections::hash_map::Entry::Vacant(entry) => {
        entry.insert(CssModuleExport {
          name: self.config.pattern.write_to_string(hash, name).unwrap(),
          composes: vec![],
          is_referenced: true,
        });
      }
    }
  }

  pub fn handle_composes(
    &mut self,
    selectors: &SelectorList<Selectors>,
    composes: &Composes,
  ) -> Result<(), PrinterErrorKind> {
    let hash = &self.hash;
    for sel in &selectors.0 {
      if sel.len() == 1 {
        match sel.iter_raw_match_order().next().unwrap() {
          parcel_selectors::parser::Component::Class(ref id) => {
            for name in &composes.names {
              let reference = match &composes.from {
                None => CssModuleReference::Local {
                  name: self.config.pattern.write_to_string(hash, name.0.as_ref()).unwrap(),
                },
                Some(ComposesFrom::Global) => CssModuleReference::Global {
                  name: name.0.as_ref().into(),
                },
                Some(ComposesFrom::File(file)) => CssModuleReference::Dependency {
                  name: name.0.to_string(),
                  specifier: file.to_string(),
                },
              };

              let export = self.exports.get_mut(&id.0.as_ref().to_owned()).unwrap();
              if !export.composes.contains(&reference) {
                export.composes.push(reference);
              }
            }
            continue;
          }
          _ => {}
        }
      }

      // The composes property can only be used within a simple class selector.
      return Err(PrinterErrorKind::InvalidComposesSelector);
    }

    Ok(())
  }
}

fn get_hashed_name(hash: &str, name: &str) -> String {
  // Hash must come first so that CSS grid identifiers work.
  // This is because grid lines may have an implicit -start or -end appended.
  format!("{}_{}", hash, name)
}

pub(crate) fn hash(s: &str) -> String {
  let mut hasher = DefaultHasher::new();
  s.hash(&mut hasher);
  let hash = hasher.finish() as u32;

  let hash = ENCODER.encode(&hash.to_le_bytes());
  if matches!(hash.as_bytes()[0], b'0'..=b'9') {
    format!("_{}", hash)
  } else {
    hash
  }
}
