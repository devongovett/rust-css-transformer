/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use crate::attr::{AttrSelectorOperator, AttrSelectorWithOptionalNamespace};
use crate::attr::{NamespaceConstraint, ParsedAttrSelectorOperation};
use crate::attr::{ParsedCaseSensitivity, SELECTOR_WHITESPACE};
use crate::bloom::BLOOM_HASH_MASK;
use crate::builder::{SelectorBuilder, SelectorFlags, SpecificityAndFlags};
use crate::context::QuirksMode;
use crate::sink::Push;
pub use crate::visitor::SelectorVisitor;
use cssparser::parse_nth;
use cssparser::{BasicParseError, BasicParseErrorKind, ParseError, ParseErrorKind};
use cssparser::{CowRcStr, Delimiter, SourceLocation};
use cssparser::{Parser as CssParser, ToCss, Token};
use precomputed_hash::PrecomputedHash;
use smallvec::{smallvec, SmallVec};
use std::borrow::Borrow;
use std::fmt::{self, Debug};
use std::iter::Rev;
use std::slice;

/// A trait that represents a pseudo-element.
pub trait PseudoElement<'i>: Sized + ToCss {
  /// The `SelectorImpl` this pseudo-element is used for.
  type Impl: SelectorImpl<'i>;

  /// Whether the pseudo-element supports a given state selector to the right
  /// of it.
  fn accepts_state_pseudo_classes(&self) -> bool {
    false
  }

  /// Whether this pseudo-element is valid after a ::slotted(..) pseudo.
  fn valid_after_slotted(&self) -> bool {
    false
  }

  fn is_webkit_scrollbar(&self) -> bool {
    false
  }

  fn is_view_transition(&self) -> bool {
    false
  }

  fn is_unknown(&self) -> bool {
    false
  }
}

/// A trait that represents a pseudo-class.
pub trait NonTSPseudoClass<'i>: Sized + ToCss {
  /// The `SelectorImpl` this pseudo-element is used for.
  type Impl: SelectorImpl<'i>;

  /// Whether this pseudo-class is :active or :hover.
  fn is_active_or_hover(&self) -> bool;

  /// Whether this pseudo-class belongs to:
  ///
  /// https://drafts.csswg.org/selectors-4/#useraction-pseudos
  fn is_user_action_state(&self) -> bool;

  fn is_valid_before_webkit_scrollbar(&self) -> bool {
    true
  }

  fn is_valid_after_webkit_scrollbar(&self) -> bool {
    false
  }

  fn visit<V>(&self, _visitor: &mut V) -> bool
  where
    V: SelectorVisitor<'i, Impl = Self::Impl>,
  {
    true
  }
}

/// Returns a Cow::Borrowed if `s` is already ASCII lowercase, and a
/// Cow::Owned if `s` had to be converted into ASCII lowercase.
fn to_ascii_lowercase<'i>(s: CowRcStr<'i>) -> CowRcStr<'i> {
  if let Some(first_uppercase) = s.bytes().position(|byte| byte >= b'A' && byte <= b'Z') {
    let mut string = s.to_string();
    string[first_uppercase..].make_ascii_lowercase();
    string.into()
  } else {
    s
  }
}

bitflags! {
    /// Flags that indicate at which point of parsing a selector are we.
    #[derive(PartialEq, Eq, Clone, Copy)]
    struct SelectorParsingState: u16 {
        /// Whether we should avoid adding default namespaces to selectors that
        /// aren't type or universal selectors.
        const SKIP_DEFAULT_NAMESPACE = 1 << 0;

        /// Whether we've parsed a ::slotted() pseudo-element already.
        ///
        /// If so, then we can only parse a subset of pseudo-elements, and
        /// whatever comes after them if so.
        const AFTER_SLOTTED = 1 << 1;
        /// Whether we've parsed a ::part() pseudo-element already.
        ///
        /// If so, then we can only parse a subset of pseudo-elements, and
        /// whatever comes after them if so.
        const AFTER_PART = 1 << 2;
        /// Whether we've parsed a pseudo-element (as in, an
        /// `Impl::PseudoElement` thus not accounting for `::slotted` or
        /// `::part`) already.
        ///
        /// If so, then other pseudo-elements and most other selectors are
        /// disallowed.
        const AFTER_PSEUDO_ELEMENT = 1 << 3;
        /// Whether we've parsed a non-stateful pseudo-element (again, as-in
        /// `Impl::PseudoElement`) already. If so, then other pseudo-classes are
        /// disallowed. If this flag is set, `AFTER_PSEUDO_ELEMENT` must be set
        /// as well.
        const AFTER_NON_STATEFUL_PSEUDO_ELEMENT = 1 << 4;

        /// Whether we are after any of the pseudo-like things.
        const AFTER_PSEUDO = Self::AFTER_PART.bits() | Self::AFTER_SLOTTED.bits() | Self::AFTER_PSEUDO_ELEMENT.bits();

        /// Whether we explicitly disallow combinators.
        const DISALLOW_COMBINATORS = 1 << 5;

        /// Whether we explicitly disallow pseudo-element-like things.
        const DISALLOW_PSEUDOS = 1 << 6;

        /// Whether we have seen a nesting selector.
        const AFTER_NESTING = 1 << 7;

        const AFTER_WEBKIT_SCROLLBAR = 1 << 8;
        const AFTER_VIEW_TRANSITION = 1 << 9;
        const AFTER_UNKNOWN_PSEUDO_ELEMENT = 1 << 10;
    }
}

impl SelectorParsingState {
  #[inline]
  fn allows_pseudos(self) -> bool {
    // NOTE(emilio): We allow pseudos after ::part and such.
    !self.intersects(Self::AFTER_PSEUDO_ELEMENT | Self::DISALLOW_PSEUDOS)
  }

  #[inline]
  fn allows_slotted(self) -> bool {
    !self.intersects(Self::AFTER_PSEUDO | Self::DISALLOW_PSEUDOS)
  }

  #[inline]
  fn allows_part(self) -> bool {
    !self.intersects(Self::AFTER_PSEUDO | Self::DISALLOW_PSEUDOS)
  }

  // TODO(emilio): Maybe some of these should be allowed, but this gets us on
  // the safe side for now, matching previous behavior. Gotta be careful with
  // the ones like :-moz-any, which allow nested selectors but don't carry the
  // state, and so on.
  #[inline]
  fn allows_custom_functional_pseudo_classes(self) -> bool {
    !self.intersects(Self::AFTER_PSEUDO)
  }

  #[inline]
  fn allows_non_functional_pseudo_classes(self) -> bool {
    !self.intersects(Self::AFTER_SLOTTED | Self::AFTER_NON_STATEFUL_PSEUDO_ELEMENT)
  }

  #[inline]
  fn allows_tree_structural_pseudo_classes(self) -> bool {
    !self.intersects(Self::AFTER_PSEUDO)
  }

  #[inline]
  fn allows_combinators(self) -> bool {
    !self.intersects(Self::DISALLOW_COMBINATORS)
  }
}

pub type SelectorParseError<'i> = ParseError<'i, SelectorParseErrorKind<'i>>;

#[derive(Clone, Debug, PartialEq)]
pub enum SelectorParseErrorKind<'i> {
  NoQualifiedNameInAttributeSelector(Token<'i>),
  EmptySelector,
  DanglingCombinator,
  InvalidPseudoClassBeforeWebKitScrollbar,
  InvalidPseudoClassAfterWebKitScrollbar,
  InvalidPseudoClassAfterPseudoElement,
  InvalidState,
  MissingNestingSelector,
  MissingNestingPrefix,
  UnexpectedTokenInAttributeSelector(Token<'i>),
  PseudoElementExpectedIdent(Token<'i>),
  UnsupportedPseudoElement(CowRcStr<'i>),
  UnsupportedPseudoClass(CowRcStr<'i>),
  AmbiguousCssModuleClass(CowRcStr<'i>),
  UnexpectedIdent(CowRcStr<'i>),
  ExpectedNamespace(CowRcStr<'i>),
  ExpectedBarInAttr(Token<'i>),
  BadValueInAttr(Token<'i>),
  InvalidQualNameInAttr(Token<'i>),
  ExplicitNamespaceUnexpectedToken(Token<'i>),
  ClassNeedsIdent(Token<'i>),
  UnexpectedSelectorAfterPseudoElement(Token<'i>),
}

macro_rules! with_all_bounds {
    (
        [ $( $InSelector: tt )* ]
        [ $( $CommonBounds: tt )* ]
        [ $( $FromStr: tt )* ]
    ) => {
        /// This trait allows to define the parser implementation in regards
        /// of pseudo-classes/elements
        ///
        /// NB: We need Clone so that we can derive(Clone) on struct with that
        /// are parameterized on SelectorImpl. See
        /// <https://github.com/rust-lang/rust/issues/26925>
        pub trait SelectorImpl<'i>: Clone + Debug + Sized + 'static {
            type ExtraMatchingData: Sized + Default + 'static;
            type AttrValue: $($InSelector)*;
            type Identifier: $($InSelector)*;
            type LocalName: $($InSelector)* + Borrow<Self::BorrowedLocalName>;
            type NamespaceUrl: $($CommonBounds)* + $($FromStr)* + Default + Borrow<Self::BorrowedNamespaceUrl>;
            type NamespacePrefix: $($InSelector)* + Default;
            type BorrowedNamespaceUrl: ?Sized + Eq;
            type BorrowedLocalName: ?Sized + Eq;

            /// non tree-structural pseudo-classes
            /// (see: https://drafts.csswg.org/selectors/#structural-pseudos)
            type NonTSPseudoClass: $($CommonBounds)* + NonTSPseudoClass<'i, Impl = Self>;
            type VendorPrefix: Sized + Eq + $($CommonBounds)* + ToCss;

            /// pseudo-elements
            type PseudoElement: $($CommonBounds)* + PseudoElement<'i, Impl = Self>;

            fn to_css<W: fmt::Write>(selectors: &SelectorList<'i, Self>, dest: &mut W) -> fmt::Result {
                serialize_selector_list(selectors.0.iter(), dest)
            }
        }
    }
}

macro_rules! with_bounds {
    ( [ $( $CommonBounds: tt )* ] [ $( $FromStr: tt )* ]) => {
        with_all_bounds! {
            [$($CommonBounds)* + $($FromStr)* + ToCss]
            [$($CommonBounds)*]
            [$($FromStr)*]
        }
    }
}

#[cfg(feature = "serde")]
with_bounds! {
    [Clone + PartialEq + Eq + std::hash::Hash]
    [From<CowRcStr<'i>> + From<std::borrow::Cow<'i, str>> + AsRef<str>]
}

#[cfg(not(feature = "serde"))]
with_bounds! {
    [Clone + PartialEq + Eq + std::hash::Hash]
    [From<CowRcStr<'i>>]
}

pub trait Parser<'i> {
  type Impl: SelectorImpl<'i>;
  type Error: 'i + From<SelectorParseErrorKind<'i>>;

  /// Whether to parse the `::slotted()` pseudo-element.
  fn parse_slotted(&self) -> bool {
    false
  }

  /// Whether to parse the `::part()` pseudo-element.
  fn parse_part(&self) -> bool {
    false
  }

  /// Whether to parse the `:where` pseudo-class.
  fn parse_is_and_where(&self) -> bool {
    false
  }

  /// The error recovery that selector lists inside :is() and :where() have.
  fn is_and_where_error_recovery(&self) -> ParseErrorRecovery {
    ParseErrorRecovery::IgnoreInvalidSelector
  }

  /// Whether the given function name is an alias for the `:is()` function.
  fn parse_any_prefix(&self, _name: &str) -> Option<<Self::Impl as SelectorImpl<'i>>::VendorPrefix> {
    None
  }

  /// Whether to parse the `:host` pseudo-class.
  fn parse_host(&self) -> bool {
    false
  }

  /// Parses non-tree-structural pseudo-classes. Tree structural pseudo-classes,
  /// like `:first-child`, are built into this library.
  ///
  /// This function can return an "Err" pseudo-element in order to support CSS2.1
  /// pseudo-elements.
  fn parse_non_ts_pseudo_class(
    &self,
    location: SourceLocation,
    name: CowRcStr<'i>,
  ) -> Result<<Self::Impl as SelectorImpl<'i>>::NonTSPseudoClass, ParseError<'i, Self::Error>> {
    Err(location.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoClass(name)))
  }

  fn parse_non_ts_functional_pseudo_class<'t>(
    &self,
    name: CowRcStr<'i>,
    arguments: &mut CssParser<'i, 't>,
  ) -> Result<<Self::Impl as SelectorImpl<'i>>::NonTSPseudoClass, ParseError<'i, Self::Error>> {
    Err(arguments.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoClass(name)))
  }

  fn parse_pseudo_element(
    &self,
    location: SourceLocation,
    name: CowRcStr<'i>,
  ) -> Result<<Self::Impl as SelectorImpl<'i>>::PseudoElement, ParseError<'i, Self::Error>> {
    Err(location.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoElement(name)))
  }

  fn parse_functional_pseudo_element<'t>(
    &self,
    name: CowRcStr<'i>,
    arguments: &mut CssParser<'i, 't>,
  ) -> Result<<Self::Impl as SelectorImpl<'i>>::PseudoElement, ParseError<'i, Self::Error>> {
    Err(arguments.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoElement(name)))
  }

  fn default_namespace(&self) -> Option<<Self::Impl as SelectorImpl<'i>>::NamespaceUrl> {
    None
  }

  fn namespace_for_prefix(
    &self,
    _prefix: &<Self::Impl as SelectorImpl<'i>>::NamespacePrefix,
  ) -> Option<<Self::Impl as SelectorImpl<'i>>::NamespaceUrl> {
    None
  }

  fn is_nesting_allowed(&self) -> bool {
    false
  }

  fn deep_combinator_enabled(&self) -> bool {
    false
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(bound(
    serialize = "Impl::NonTSPseudoClass: serde::Serialize, Impl::PseudoElement: serde::Serialize, Impl::VendorPrefix: serde::Serialize",
    deserialize = "Impl::NonTSPseudoClass: serde::Deserialize<'de>, Impl::PseudoElement: serde::Deserialize<'de>, Impl::VendorPrefix: serde::Deserialize<'de>"
  ))
)]
#[cfg_attr(
  feature = "jsonschema",
  derive(schemars::JsonSchema),
  schemars(
    rename = "SelectorList",
    bound = "Impl: schemars::JsonSchema, Impl::NonTSPseudoClass: schemars::JsonSchema, Impl::PseudoElement: schemars::JsonSchema, Impl::VendorPrefix: schemars::JsonSchema"
  )
)]
pub struct SelectorList<'i, Impl: SelectorImpl<'i>>(
  #[cfg_attr(feature = "serde", serde(borrow))] pub SmallVec<[Selector<'i, Impl>; 1]>,
);

#[cfg(feature = "into_owned")]
impl<'any, 'i, Impl: SelectorImpl<'i>, NewSel> static_self::IntoOwned<'any> for SelectorList<'i, Impl>
where
  Impl: static_self::IntoOwned<'any, Owned = NewSel>,
  NewSel: SelectorImpl<'any>,
  Component<'i, Impl>: static_self::IntoOwned<'any, Owned = Component<'any, NewSel>>,
{
  type Owned = SelectorList<'any, NewSel>;

  fn into_owned(self) -> Self::Owned {
    SelectorList(self.0.into_owned())
  }
}

/// How to treat invalid selectors in a selector list.
pub enum ParseErrorRecovery {
  /// Discard the entire selector list, this is the default behavior for
  /// almost all of CSS.
  DiscardList,
  /// Ignore invalid selectors, potentially creating an empty selector list.
  ///
  /// This is the error recovery mode of :is() and :where()
  IgnoreInvalidSelector,
}

#[derive(Eq, PartialEq, Clone, Copy)]
pub enum NestingRequirement {
  None,
  Prefixed,
  Contained,
  Implicit,
}

impl<'i, Impl: SelectorImpl<'i>> SelectorList<'i, Impl> {
  /// Parse a comma-separated list of Selectors.
  /// <https://drafts.csswg.org/selectors/#grouping>
  ///
  /// Return the Selectors or Err if there is an invalid selector.
  pub fn parse<'t, P>(
    parser: &P,
    input: &mut CssParser<'i, 't>,
    error_recovery: ParseErrorRecovery,
    nesting_requirement: NestingRequirement,
  ) -> Result<Self, ParseError<'i, P::Error>>
  where
    P: Parser<'i, Impl = Impl>,
  {
    Self::parse_with_state(
      parser,
      input,
      &mut SelectorParsingState::empty(),
      error_recovery,
      nesting_requirement,
    )
  }

  #[inline]
  fn parse_with_state<'t, P>(
    parser: &P,
    input: &mut CssParser<'i, 't>,
    state: &mut SelectorParsingState,
    recovery: ParseErrorRecovery,
    nesting_requirement: NestingRequirement,
  ) -> Result<Self, ParseError<'i, P::Error>>
  where
    P: Parser<'i, Impl = Impl>,
  {
    let original_state = *state;
    let mut values = SmallVec::new();
    loop {
      let selector = input.parse_until_before(Delimiter::Comma, |input| {
        let mut selector_state = original_state;
        let result = parse_selector(parser, input, &mut selector_state, nesting_requirement);
        if selector_state.contains(SelectorParsingState::AFTER_NESTING) {
          state.insert(SelectorParsingState::AFTER_NESTING)
        }
        result
      });

      let was_ok = selector.is_ok();
      match selector {
        Ok(selector) => values.push(selector),
        Err(err) => match recovery {
          ParseErrorRecovery::DiscardList => return Err(err),
          ParseErrorRecovery::IgnoreInvalidSelector => {}
        },
      }

      loop {
        match input.next() {
          Err(_) => return Ok(SelectorList(values)),
          Ok(&Token::Comma) => break,
          Ok(_) => {
            debug_assert!(!was_ok, "Shouldn't have got a selector if getting here");
          }
        }
      }
    }
  }

  pub fn parse_relative<'t, P>(
    parser: &P,
    input: &mut CssParser<'i, 't>,
    error_recovery: ParseErrorRecovery,
    nesting_requirement: NestingRequirement,
  ) -> Result<Self, ParseError<'i, P::Error>>
  where
    P: Parser<'i, Impl = Impl>,
  {
    Self::parse_relative_with_state(
      parser,
      input,
      &mut SelectorParsingState::empty(),
      error_recovery,
      nesting_requirement,
    )
  }

  #[inline]
  fn parse_relative_with_state<'t, P>(
    parser: &P,
    input: &mut CssParser<'i, 't>,
    state: &mut SelectorParsingState,
    recovery: ParseErrorRecovery,
    nesting_requirement: NestingRequirement,
  ) -> Result<Self, ParseError<'i, P::Error>>
  where
    P: Parser<'i, Impl = Impl>,
  {
    let original_state = *state;
    let mut values = SmallVec::new();
    loop {
      let selector = input.parse_until_before(Delimiter::Comma, |input| {
        let mut selector_state = original_state;
        let result = parse_relative_selector(parser, input, &mut selector_state, nesting_requirement);
        if selector_state.contains(SelectorParsingState::AFTER_NESTING) {
          state.insert(SelectorParsingState::AFTER_NESTING)
        }
        result
      });

      let was_ok = selector.is_ok();
      match selector {
        Ok(selector) => values.push(selector),
        Err(err) => match recovery {
          ParseErrorRecovery::DiscardList => return Err(err),
          ParseErrorRecovery::IgnoreInvalidSelector => {}
        },
      }

      loop {
        match input.next() {
          Err(_) => return Ok(SelectorList(values)),
          Ok(&Token::Comma) => break,
          Ok(_) => {
            debug_assert!(!was_ok, "Shouldn't have got a selector if getting here");
          }
        }
      }
    }
  }

  /// Creates a new SelectorList.
  pub fn new(v: SmallVec<[Selector<'i, Impl>; 1]>) -> Self {
    SelectorList(v)
  }

  /// Creates a SelectorList from a Vec of selectors. Used in tests.
  pub fn from_vec(v: Vec<Selector<'i, Impl>>) -> Self {
    SelectorList(SmallVec::from_vec(v))
  }
}

impl<'i, Impl: SelectorImpl<'i>> From<Selector<'i, Impl>> for SelectorList<'i, Impl> {
  fn from(selector: Selector<'i, Impl>) -> Self {
    SelectorList(smallvec![selector])
  }
}

impl<'i, Impl: SelectorImpl<'i>> From<Component<'i, Impl>> for SelectorList<'i, Impl> {
  fn from(component: Component<'i, Impl>) -> Self {
    SelectorList::from(Selector::from(component))
  }
}

/// Parses one compound selector suitable for nested stuff like :-moz-any, etc.
fn parse_inner_compound_selector<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  state: &mut SelectorParsingState,
) -> Result<Selector<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  let mut child_state =
    *state | SelectorParsingState::DISALLOW_PSEUDOS | SelectorParsingState::DISALLOW_COMBINATORS;
  let result = parse_selector(parser, input, &mut child_state, NestingRequirement::None)?;
  if child_state.contains(SelectorParsingState::AFTER_NESTING) {
    state.insert(SelectorParsingState::AFTER_NESTING)
  }
  Ok(result)
}

/// Ancestor hashes for the bloom filter. We precompute these and store them
/// inline with selectors to optimize cache performance during matching.
/// This matters a lot.
///
/// We use 4 hashes, which is copied from Gecko, who copied it from WebKit.
/// Note that increasing the number of hashes here will adversely affect the
/// cache hit when fast-rejecting long lists of Rules with inline hashes.
///
/// Because the bloom filter only uses the bottom 24 bits of the hash, we pack
/// the fourth hash into the upper bits of the first three hashes in order to
/// shrink Rule (whose size matters a lot). This scheme minimizes the runtime
/// overhead of the packing for the first three hashes (we just need to mask
/// off the upper bits) at the expense of making the fourth somewhat more
/// complicated to assemble, because we often bail out before checking all the
/// hashes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AncestorHashes {
  pub packed_hashes: [u32; 3],
}

fn collect_ancestor_hashes<'i, Impl: SelectorImpl<'i>>(
  iter: SelectorIter<'_, 'i, Impl>,
  quirks_mode: QuirksMode,
  hashes: &mut [u32; 4],
  len: &mut usize,
) -> bool
where
  Impl::Identifier: PrecomputedHash,
  Impl::LocalName: PrecomputedHash,
  Impl::NamespaceUrl: PrecomputedHash,
{
  for component in AncestorIter::new(iter) {
    let hash = match *component {
      Component::LocalName(LocalName {
        ref name,
        ref lower_name,
      }) => {
        // Only insert the local-name into the filter if it's all
        // lowercase.  Otherwise we would need to test both hashes, and
        // our data structures aren't really set up for that.
        if name != lower_name {
          continue;
        }
        name.precomputed_hash()
      }
      Component::DefaultNamespace(ref url) | Component::Namespace(_, ref url) => url.precomputed_hash(),
      // In quirks mode, class and id selectors should match
      // case-insensitively, so just avoid inserting them into the filter.
      Component::ID(ref id) if quirks_mode != QuirksMode::Quirks => id.precomputed_hash(),
      Component::Class(ref class) if quirks_mode != QuirksMode::Quirks => class.precomputed_hash(),
      Component::Is(ref list) | Component::Where(ref list) => {
        // :where and :is OR their selectors, so we can't put any hash
        // in the filter if there's more than one selector, as that'd
        // exclude elements that may match one of the other selectors.
        if list.len() == 1 && !collect_ancestor_hashes(list[0].iter(), quirks_mode, hashes, len) {
          return false;
        }
        continue;
      }
      _ => continue,
    };

    hashes[*len] = hash & BLOOM_HASH_MASK;
    *len += 1;
    if *len == hashes.len() {
      return false;
    }
  }
  true
}

impl AncestorHashes {
  pub fn new<'i, Impl: SelectorImpl<'i>>(selector: &Selector<'i, Impl>, quirks_mode: QuirksMode) -> Self
  where
    Impl::Identifier: PrecomputedHash,
    Impl::LocalName: PrecomputedHash,
    Impl::NamespaceUrl: PrecomputedHash,
  {
    // Compute ancestor hashes for the bloom filter.
    let mut hashes = [0u32; 4];
    let mut len = 0;
    collect_ancestor_hashes(selector.iter(), quirks_mode, &mut hashes, &mut len);
    debug_assert!(len <= 4);

    // Now, pack the fourth hash (if it exists) into the upper byte of each of
    // the other three hashes.
    if len == 4 {
      let fourth = hashes[3];
      hashes[0] |= (fourth & 0x000000ff) << 24;
      hashes[1] |= (fourth & 0x0000ff00) << 16;
      hashes[2] |= (fourth & 0x00ff0000) << 8;
    }

    AncestorHashes {
      packed_hashes: [hashes[0], hashes[1], hashes[2]],
    }
  }

  /// Returns the fourth hash, reassembled from parts.
  pub fn fourth_hash(&self) -> u32 {
    ((self.packed_hashes[0] & 0xff000000) >> 24)
      | ((self.packed_hashes[1] & 0xff000000) >> 16)
      | ((self.packed_hashes[2] & 0xff000000) >> 8)
  }
}

pub fn namespace_empty_string<'i, Impl: SelectorImpl<'i>>() -> Impl::NamespaceUrl {
  // Rust type’s default, not default namespace
  Impl::NamespaceUrl::default()
}

/// A Selector stores a sequence of simple selectors and combinators. The
/// iterator classes allow callers to iterate at either the raw sequence level or
/// at the level of sequences of simple selectors separated by combinators. Most
/// callers want the higher-level iterator.
///
/// We store compound selectors internally right-to-left (in matching order).
/// Additionally, we invert the order of top-level compound selectors so that
/// each one matches left-to-right. This is because matching namespace, local name,
/// id, and class are all relatively cheap, whereas matching pseudo-classes might
/// be expensive (depending on the pseudo-class). Since authors tend to put the
/// pseudo-classes on the right, it's faster to start matching on the left.
///
/// This reordering doesn't change the semantics of selector matching, and we
/// handle it in to_css to make it invisible to serialization.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Selector<'i, Impl: SelectorImpl<'i>>(SpecificityAndFlags, Vec<Component<'i, Impl>>);

#[cfg(feature = "into_owned")]
impl<'any, 'i, Impl: SelectorImpl<'i>, NewSel> static_self::IntoOwned<'any> for Selector<'i, Impl>
where
  Impl: static_self::IntoOwned<'any, Owned = NewSel>,
  NewSel: SelectorImpl<'any>,
  Component<'i, Impl>: static_self::IntoOwned<'any, Owned = Component<'any, NewSel>>,
{
  type Owned = Selector<'any, NewSel>;

  fn into_owned(self) -> Self::Owned {
    Selector(self.0, self.1.into_owned())
  }
}

impl<'i, Impl: SelectorImpl<'i>> Selector<'i, Impl> {
  #[inline]
  pub fn specificity(&self) -> u32 {
    self.0.specificity()
  }

  #[inline]
  pub fn has_pseudo_element(&self) -> bool {
    self.0.has_pseudo_element()
  }

  #[inline]
  pub fn is_slotted(&self) -> bool {
    self.0.is_slotted()
  }

  #[inline]
  pub fn is_part(&self) -> bool {
    self.0.is_part()
  }

  #[inline]
  pub fn append(&mut self, component: Component<'i, Impl>) {
    let index = self
      .1
      .iter()
      .position(|c| matches!(*c, Component::Combinator(..) | Component::PseudoElement(..)))
      .unwrap_or(self.1.len());
    self.1.insert(index, component);
  }

  /// Inserts a [`Component`] to an arbitrary index in the internal components Vec.
  ///
  /// An order in which components are stored is right-to-left (see [`Selector::iter_raw_match_order()`])
  #[inline]
  pub fn insert_raw(&mut self, index: usize, component: Component<'i, Impl>) {
    self.1.insert(index, component)
  }

  /// Inserts multiple [`Component`]s to an arbitrary index in the internal components Vec.
  ///
  /// An order in which components are stored is right-to-left (see [`Selector::iter_raw_match_order()`])
  #[inline]
  pub fn insert_raw_multiple(&mut self, index: usize, mut components: Vec<Component<'i, Impl>>) {
    if index > self.1.len() {
      return;
    }

    let old_len = self.1.len();
    let mut old = std::mem::replace(&mut self.1, Vec::with_capacity(old_len + components.len()));

    // Fast track: insert at index 0 is the same as Vec::append
    if index == 0 {
      self.1.append(&mut components);
      self.1.append(&mut old);
      return;
    }

    // Manually move the first `index` elements
    let mut old_iter = old.into_iter();
    for _ in 0..index {
      let Some(old_component) = old_iter.next() else {
        unreachable!("index is never greater than selector len")
      };

      self.1.push(old_component)
    }

    self.1.append(&mut components);
    self.1.extend(old_iter);
  }

  #[inline]
  pub fn parts(&self) -> Option<&[Impl::Identifier]> {
    if !self.is_part() {
      return None;
    }

    let mut iter = self.iter();
    if self.has_pseudo_element() {
      // Skip the pseudo-element.
      for _ in &mut iter {}

      let combinator = iter.next_sequence()?;
      debug_assert_eq!(combinator, Combinator::PseudoElement);
    }

    for component in iter {
      if let Component::Part(ref part) = *component {
        return Some(part);
      }
    }

    debug_assert!(false, "is_part() lied somehow?");
    None
  }

  #[inline]
  pub fn pseudo_element(&self) -> Option<&Impl::PseudoElement> {
    if !self.has_pseudo_element() {
      return None;
    }

    for component in self.iter() {
      if let Component::PseudoElement(ref pseudo) = *component {
        return Some(pseudo);
      }
    }

    debug_assert!(false, "has_pseudo_element lied!");
    None
  }

  /// Whether this selector (pseudo-element part excluded) matches every element.
  ///
  /// Used for "pre-computed" pseudo-elements in components/style/stylist.rs
  #[inline]
  pub fn is_universal(&self) -> bool {
    self.iter_raw_match_order().all(|c| {
      matches!(
        *c,
        Component::ExplicitUniversalType
          | Component::ExplicitAnyNamespace
          | Component::Combinator(Combinator::PseudoElement)
          | Component::PseudoElement(..)
      )
    })
  }

  #[inline]
  pub fn has_combinator(&self) -> bool {
    self
      .iter_raw_match_order()
      .any(|c| matches!(*c, Component::Combinator(combinator) if combinator.is_tree_combinator()))
  }

  /// Returns an iterator over this selector in matching order (right-to-left).
  /// When a combinator is reached, the iterator will return None, and
  /// next_sequence() may be called to continue to the next sequence.
  #[inline]
  pub fn iter(&self) -> SelectorIter<'_, 'i, Impl> {
    SelectorIter {
      iter: self.iter_raw_match_order(),
      next_combinator: None,
    }
  }

  /// Whether this selector is a featureless :host selector, with no
  /// combinators to the left, and optionally has a pseudo-element to the
  /// right.
  #[inline]
  pub fn is_featureless_host_selector_or_pseudo_element(&self) -> bool {
    let mut iter = self.iter();
    if !self.has_pseudo_element() {
      return iter.is_featureless_host_selector();
    }

    // Skip the pseudo-element.
    for _ in &mut iter {}

    match iter.next_sequence() {
      None => return false,
      Some(combinator) => {
        debug_assert_eq!(combinator, Combinator::PseudoElement);
      }
    }

    iter.is_featureless_host_selector()
  }

  /// Returns an iterator over this selector in matching order (right-to-left),
  /// skipping the rightmost |offset| Components.
  #[inline]
  pub fn iter_from(&self, offset: usize) -> SelectorIter<'_, 'i, Impl> {
    let iter = self.1[offset..].iter();
    SelectorIter {
      iter,
      next_combinator: None,
    }
  }

  /// Returns the combinator at index `index` (zero-indexed from the right),
  /// or panics if the component is not a combinator.
  #[inline]
  pub fn combinator_at_match_order(&self, index: usize) -> Combinator {
    match self.1[index] {
      Component::Combinator(c) => c,
      ref other => panic!("Not a combinator: {:?}, {:?}, index: {}", other, self, index),
    }
  }

  /// Returns an iterator over the entire sequence of simple selectors and
  /// combinators, in matching order (from right to left).
  #[inline]
  pub fn iter_raw_match_order(&self) -> slice::Iter<Component<'i, Impl>> {
    self.1.iter()
  }

  #[inline]
  pub fn iter_mut_raw_match_order(&mut self) -> slice::IterMut<Component<'i, Impl>> {
    self.1.iter_mut()
  }

  /// Returns the combinator at index `index` (zero-indexed from the left),
  /// or panics if the component is not a combinator.
  #[inline]
  pub fn combinator_at_parse_order(&self, index: usize) -> Combinator {
    match self.1[self.len() - index - 1] {
      Component::Combinator(c) => c,
      ref other => panic!("Not a combinator: {:?}, {:?}, index: {}", other, self, index),
    }
  }

  /// Returns an iterator over the sequence of simple selectors and
  /// combinators, in parse order (from left to right), starting from
  /// `offset`.
  #[inline]
  pub fn iter_raw_parse_order_from(&self, offset: usize) -> Rev<slice::Iter<Component<'i, Impl>>> {
    self.1[..self.len() - offset].iter().rev()
  }

  /// Creates a Selector from a vec of Components, specified in parse order. Used in tests.
  #[allow(unused)]
  pub(crate) fn from_vec(vec: Vec<Component<'i, Impl>>, specificity: u32, flags: SelectorFlags) -> Self {
    let mut builder = SelectorBuilder::default();
    for component in vec.into_iter() {
      if let Some(combinator) = component.as_combinator() {
        builder.push_combinator(combinator);
      } else {
        builder.push_simple_selector(component);
      }
    }
    let spec = SpecificityAndFlags { specificity, flags };
    let (spec, components) = builder.build_with_specificity_and_flags(spec);
    Selector(spec, components)
  }

  #[cfg(feature = "serde")]
  #[inline]
  pub(crate) fn new(spec: SpecificityAndFlags, components: Vec<Component<'i, Impl>>) -> Self {
    Selector(spec, components)
  }

  /// Returns count of simple selectors and combinators in the Selector.
  #[inline]
  pub fn len(&self) -> usize {
    self.1.len()
  }

  /// Traverse selector components inside `self`.
  ///
  /// Implementations of this method should call `SelectorVisitor` methods
  /// or other impls of `Visit` as appropriate based on the fields of `Self`.
  ///
  /// A return value of `false` indicates terminating the traversal.
  /// It should be propagated with an early return.
  /// On the contrary, `true` indicates that all fields of `self` have been traversed:
  ///
  /// ```rust,ignore
  /// if !visitor.visit_simple_selector(&self.some_simple_selector) {
  ///     return false;
  /// }
  /// if !self.some_component.visit(visitor) {
  ///     return false;
  /// }
  /// true
  /// ```
  pub fn visit<V>(&self, visitor: &mut V) -> bool
  where
    V: SelectorVisitor<'i, Impl = Impl>,
  {
    let mut current = self.iter();
    let mut combinator = None;
    loop {
      if !visitor.visit_complex_selector(combinator) {
        return false;
      }

      for selector in &mut current {
        if !selector.visit(visitor) {
          return false;
        }
      }

      combinator = current.next_sequence();
      if combinator.is_none() {
        break;
      }
    }

    true
  }
}

impl<'i, Impl: SelectorImpl<'i>> From<Component<'i, Impl>> for Selector<'i, Impl> {
  fn from(component: Component<'i, Impl>) -> Self {
    let mut builder = SelectorBuilder::default();
    if let Some(combinator) = component.as_combinator() {
      builder.push_combinator(combinator);
    } else {
      builder.push_simple_selector(component);
    }
    let (spec, components) = builder.build(false, false, false);
    Selector(spec, components)
  }
}

impl<'i, Impl: SelectorImpl<'i>> From<Vec<Component<'i, Impl>>> for Selector<'i, Impl> {
  fn from(vec: Vec<Component<'i, Impl>>) -> Self {
    let mut builder = SelectorBuilder::default();
    for component in vec.into_iter() {
      if let Some(combinator) = component.as_combinator() {
        builder.push_combinator(combinator);
      } else {
        builder.push_simple_selector(component);
      }
    }
    let (spec, components) = builder.build(false, false, false);
    Selector(spec, components)
  }
}

#[derive(Clone)]
pub struct SelectorIter<'a, 'i, Impl: SelectorImpl<'i>> {
  iter: slice::Iter<'a, Component<'i, Impl>>,
  next_combinator: Option<Combinator>,
}

impl<'a, 'i, Impl: 'a + SelectorImpl<'i>> SelectorIter<'a, 'i, Impl> {
  /// Prepares this iterator to point to the next sequence to the left,
  /// returning the combinator if the sequence was found.
  #[inline]
  pub fn next_sequence(&mut self) -> Option<Combinator> {
    self.next_combinator.take()
  }

  /// Whether this selector is a featureless host selector, with no
  /// combinators to the left.
  #[inline]
  pub(crate) fn is_featureless_host_selector(&mut self) -> bool {
    self.selector_length() > 0
      && self.all(|component| matches!(*component, Component::Host(..)))
      && self.next_sequence().is_none()
  }

  #[inline]
  pub(crate) fn matches_for_stateless_pseudo_element(&mut self) -> bool {
    let first = match self.next() {
      Some(c) => c,
      // Note that this is the common path that we keep inline: the
      // pseudo-element not having anything to its right.
      None => return true,
    };
    self.matches_for_stateless_pseudo_element_internal(first)
  }

  #[inline(never)]
  fn matches_for_stateless_pseudo_element_internal(&mut self, first: &Component<'i, Impl>) -> bool {
    if !first.matches_for_stateless_pseudo_element() {
      return false;
    }
    for component in self {
      // The only other parser-allowed Components in this sequence are
      // state pseudo-classes, or one of the other things that can contain
      // them.
      if !component.matches_for_stateless_pseudo_element() {
        return false;
      }
    }
    true
  }

  /// Returns remaining count of the simple selectors and combinators in the Selector.
  #[inline]
  pub fn selector_length(&self) -> usize {
    self.iter.len()
  }
}

impl<'a, 'i, Impl: SelectorImpl<'i>> Iterator for SelectorIter<'a, 'i, Impl> {
  type Item = &'a Component<'i, Impl>;

  #[inline]
  fn next(&mut self) -> Option<Self::Item> {
    debug_assert!(self.next_combinator.is_none(), "You should call next_sequence!");
    match *self.iter.next()? {
      Component::Combinator(c) => {
        self.next_combinator = Some(c);
        None
      }
      ref x => Some(x),
    }
  }
}

impl<'a, 'i, Impl: SelectorImpl<'i>> fmt::Debug for SelectorIter<'a, 'i, Impl> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    let iter = self.iter.clone().rev();
    for component in iter {
      component.to_css(f)?
    }
    Ok(())
  }
}

/// An iterator over all simple selectors belonging to ancestors.
struct AncestorIter<'a, 'i, Impl: SelectorImpl<'i>>(SelectorIter<'a, 'i, Impl>);
impl<'a, 'i, Impl: 'a + SelectorImpl<'i>> AncestorIter<'a, 'i, Impl> {
  /// Creates an AncestorIter. The passed-in iterator is assumed to point to
  /// the beginning of the child sequence, which will be skipped.
  fn new(inner: SelectorIter<'a, 'i, Impl>) -> Self {
    let mut result = AncestorIter(inner);
    result.skip_until_ancestor();
    result
  }

  /// Skips a sequence of simple selectors and all subsequent sequences until
  /// a non-pseudo-element ancestor combinator is reached.
  fn skip_until_ancestor(&mut self) {
    loop {
      while self.0.next().is_some() {}
      // If this is ever changed to stop at the "pseudo-element"
      // combinator, we will need to fix the way we compute hashes for
      // revalidation selectors.
      if self
        .0
        .next_sequence()
        .map_or(true, |x| matches!(x, Combinator::Child | Combinator::Descendant))
      {
        break;
      }
    }
  }
}

impl<'a, 'i, Impl: SelectorImpl<'i>> Iterator for AncestorIter<'a, 'i, Impl> {
  type Item = &'a Component<'i, Impl>;
  fn next(&mut self) -> Option<Self::Item> {
    // Grab the next simple selector in the sequence if available.
    let next = self.0.next();
    if next.is_some() {
      return next;
    }

    // See if there are more sequences. If so, skip any non-ancestor sequences.
    if let Some(combinator) = self.0.next_sequence() {
      if !matches!(combinator, Combinator::Child | Combinator::Descendant) {
        self.skip_until_ancestor();
      }
    }

    self.0.next()
  }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[cfg_attr(
  feature = "serde",
  derive(serde::Serialize, serde::Deserialize),
  serde(rename_all = "kebab-case")
)]
#[cfg_attr(feature = "jsonschema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub enum Combinator {
  Child,        //  >
  Descendant,   // space
  NextSibling,  // +
  LaterSibling, // ~
  /// A dummy combinator we use to the left of pseudo-elements.
  ///
  /// It serializes as the empty string, and acts effectively as a child
  /// combinator in most cases.  If we ever actually start using a child
  /// combinator for this, we will need to fix up the way hashes are computed
  /// for revalidation selectors.
  PseudoElement,
  /// Another combinator used for ::slotted(), which represent the jump from
  /// a node to its assigned slot.
  SlotAssignment,
  /// Another combinator used for `::part()`, which represents the jump from
  /// the part to the containing shadow host.
  Part,

  /// Non-standard Vue >>> combinator.
  /// https://vue-loader.vuejs.org/guide/scoped-css.html#deep-selectors
  DeepDescendant,
  /// Non-standard /deep/ combinator.
  /// Appeared in early versions of the css-scoping-1 specification:
  /// https://www.w3.org/TR/2014/WD-css-scoping-1-20140403/#deep-combinator
  /// And still supported as an alias for >>> by Vue.
  Deep,
}

impl Combinator {
  /// Returns true if this combinator is a child or descendant combinator.
  #[inline]
  pub fn is_ancestor(&self) -> bool {
    matches!(
      *self,
      Combinator::Child | Combinator::Descendant | Combinator::PseudoElement | Combinator::SlotAssignment
    )
  }

  /// Returns true if this combinator is a pseudo-element combinator.
  #[inline]
  pub fn is_pseudo_element(&self) -> bool {
    matches!(*self, Combinator::PseudoElement)
  }

  /// Returns true if this combinator is a next- or later-sibling combinator.
  #[inline]
  pub fn is_sibling(&self) -> bool {
    matches!(*self, Combinator::NextSibling | Combinator::LaterSibling)
  }

  #[inline]
  pub fn is_tree_combinator(&self) -> bool {
    matches!(
      *self,
      Combinator::Child | Combinator::Descendant | Combinator::NextSibling | Combinator::LaterSibling
    )
  }
}

/// An enum for the different types of :nth- pseudoclasses
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub enum NthType {
  Child,
  LastChild,
  OnlyChild,
  OfType,
  LastOfType,
  OnlyOfType,
  Col,
  LastCol,
}

impl NthType {
  pub fn is_only(self) -> bool {
    self == Self::OnlyChild || self == Self::OnlyOfType
  }

  pub fn is_of_type(self) -> bool {
    self == Self::OfType || self == Self::LastOfType || self == Self::OnlyOfType
  }

  pub fn is_from_end(self) -> bool {
    self == Self::LastChild || self == Self::LastOfType || self == Self::LastCol
  }

  pub fn allows_of_selector(self) -> bool {
    self == Self::Child || self == Self::LastChild
  }
}

/// The properties that comprise an :nth- pseudoclass as of Selectors 3 (e.g.,
/// nth-child(An+B)).
/// https://www.w3.org/TR/selectors-3/#nth-child-pseudo
#[derive(Copy, Clone, Eq, PartialEq, Hash)]
#[cfg_attr(feature = "into_owned", derive(static_self::IntoOwned))]
pub struct NthSelectorData {
  pub ty: NthType,
  pub is_function: bool,
  pub a: i32,
  pub b: i32,
}

impl NthSelectorData {
  /// Returns selector data for :only-{child,of-type}
  #[inline]
  pub const fn only(of_type: bool) -> Self {
    Self {
      ty: if of_type {
        NthType::OnlyOfType
      } else {
        NthType::OnlyChild
      },
      is_function: false,
      a: 0,
      b: 1,
    }
  }

  /// Returns selector data for :first-{child,of-type}
  #[inline]
  pub const fn first(of_type: bool) -> Self {
    Self {
      ty: if of_type { NthType::OfType } else { NthType::Child },
      is_function: false,
      a: 0,
      b: 1,
    }
  }

  /// Returns selector data for :last-{child,of-type}
  #[inline]
  pub const fn last(of_type: bool) -> Self {
    Self {
      ty: if of_type {
        NthType::LastOfType
      } else {
        NthType::LastChild
      },
      is_function: false,
      a: 0,
      b: 1,
    }
  }

  #[inline]
  pub fn is_function(&self) -> bool {
    self.a != 0 || self.b != 1
  }

  /// Writes the beginning of the selector.
  #[inline]
  pub fn write_start<W: fmt::Write>(&self, dest: &mut W, is_function: bool) -> fmt::Result {
    dest.write_str(match self.ty {
      NthType::Child if is_function => ":nth-child(",
      NthType::Child => ":first-child",
      NthType::LastChild if is_function => ":nth-last-child(",
      NthType::LastChild => ":last-child",
      NthType::OfType if is_function => ":nth-of-type(",
      NthType::OfType => ":first-of-type",
      NthType::LastOfType if is_function => ":nth-last-of-type(",
      NthType::LastOfType => ":last-of-type",
      NthType::OnlyChild => ":only-child",
      NthType::OnlyOfType => ":only-of-type",
      NthType::Col => ":nth-col(",
      NthType::LastCol => ":nth-last-col(",
    })
  }

  /// Serialize <an+b> (part of the CSS Syntax spec, but currently only used here).
  /// <https://drafts.csswg.org/css-syntax-3/#serialize-an-anb-value>
  #[inline]
  pub fn write_affine<W: fmt::Write>(&self, dest: &mut W) -> fmt::Result {
    match (self.a, self.b) {
      (0, 0) => dest.write_char('0'),

      (1, 0) => dest.write_char('n'),
      (-1, 0) => dest.write_str("-n"),
      (_, 0) => write!(dest, "{}n", self.a),

      (2, 1) => dest.write_str("odd"),

      (0, _) => write!(dest, "{}", self.b),
      (1, _) => write!(dest, "n{:+}", self.b),
      (-1, _) => write!(dest, "-n{:+}", self.b),
      (_, _) => write!(dest, "{}n{:+}", self.a, self.b),
    }
  }
}

/// The properties that comprise an :nth- pseudoclass as of Selectors 4 (e.g.,
/// nth-child(An+B [of S]?)).
/// https://www.w3.org/TR/selectors-4/#nth-child-pseudo
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct NthOfSelectorData<'i, Impl: SelectorImpl<'i>>(NthSelectorData, Box<[Selector<'i, Impl>]>);

#[cfg(feature = "into_owned")]
impl<'any, 'i, Impl: SelectorImpl<'i>, NewSel> static_self::IntoOwned<'any> for NthOfSelectorData<'i, Impl>
where
  Impl: static_self::IntoOwned<'any, Owned = NewSel>,
  NewSel: SelectorImpl<'any>,
  Component<'i, Impl>: static_self::IntoOwned<'any, Owned = Component<'any, NewSel>>,
{
  type Owned = NthOfSelectorData<'any, NewSel>;

  fn into_owned(self) -> Self::Owned {
    NthOfSelectorData(self.0, self.1.into_owned())
  }
}

impl<'i, Impl: SelectorImpl<'i>> NthOfSelectorData<'i, Impl> {
  /// Returns selector data for :nth-{,last-}{child,of-type}(An+B [of S])
  #[inline]
  pub fn new(nth_data: NthSelectorData, selectors: Box<[Selector<'i, Impl>]>) -> Self {
    Self(nth_data, selectors)
  }

  /// Returns the An+B part of the selector
  #[inline]
  pub fn nth_data(&self) -> &NthSelectorData {
    &self.0
  }

  /// Returns the selector list part of the selector
  #[inline]
  pub fn selectors(&self) -> &[Selector<'i, Impl>] {
    &*self.1
  }

  pub fn clone_selectors(&self) -> Box<[Selector<'i, Impl>]> {
    self.1.clone()
  }
}

/// A CSS simple selector or combinator. We store both in the same enum for
/// optimal packing and cache performance, see [1].
///
/// [1] https://bugzilla.mozilla.org/show_bug.cgi?id=1357973
#[derive(Clone, PartialEq, Eq, Hash)]
pub enum Component<'i, Impl: SelectorImpl<'i>> {
  Combinator(Combinator),

  ExplicitAnyNamespace,
  ExplicitNoNamespace,
  DefaultNamespace(Impl::NamespaceUrl),
  Namespace(Impl::NamespacePrefix, Impl::NamespaceUrl),

  ExplicitUniversalType,
  LocalName(LocalName<'i, Impl>),

  ID(Impl::Identifier),
  Class(Impl::Identifier),

  AttributeInNoNamespaceExists {
    local_name: Impl::LocalName,
    local_name_lower: Impl::LocalName,
  },
  // Used only when local_name is already lowercase.
  AttributeInNoNamespace {
    local_name: Impl::LocalName,
    operator: AttrSelectorOperator,
    value: Impl::AttrValue,
    case_sensitivity: ParsedCaseSensitivity,
    never_matches: bool,
  },
  // Use a Box in the less common cases with more data to keep size_of::<Component>() small.
  AttributeOther(Box<AttrSelectorWithOptionalNamespace<'i, Impl>>),

  /// Pseudo-classes
  Negation(Box<[Selector<'i, Impl>]>),
  Root,
  Empty,
  Scope,
  Nth(NthSelectorData),
  NthOf(NthOfSelectorData<'i, Impl>),
  NonTSPseudoClass(Impl::NonTSPseudoClass),
  /// The ::slotted() pseudo-element:
  ///
  /// https://drafts.csswg.org/css-scoping/#slotted-pseudo
  ///
  /// The selector here is a compound selector, that is, no combinators.
  ///
  /// NOTE(emilio): This should support a list of selectors, but as of this
  /// writing no other browser does, and that allows them to put ::slotted()
  /// in the rule hash, so we do that too.
  ///
  /// See https://github.com/w3c/csswg-drafts/issues/2158
  Slotted(Selector<'i, Impl>),
  /// The `::part` pseudo-element.
  ///   https://drafts.csswg.org/css-shadow-parts/#part
  Part(Box<[Impl::Identifier]>),
  /// The `:host` pseudo-class:
  ///
  /// https://drafts.csswg.org/css-scoping/#host-selector
  ///
  /// NOTE(emilio): This should support a list of selectors, but as of this
  /// writing no other browser does, and that allows them to put :host()
  /// in the rule hash, so we do that too.
  ///
  /// See https://github.com/w3c/csswg-drafts/issues/2158
  Host(Option<Selector<'i, Impl>>),
  /// The `:where` pseudo-class.
  ///
  /// https://drafts.csswg.org/selectors/#zero-matches
  ///
  /// The inner argument is conceptually a SelectorList, but we move the
  /// selectors to the heap to keep Component small.
  Where(Box<[Selector<'i, Impl>]>),
  /// The `:is` pseudo-class.
  ///
  /// https://drafts.csswg.org/selectors/#matches-pseudo
  ///
  /// Same comment as above re. the argument.
  Is(Box<[Selector<'i, Impl>]>),
  Any(Impl::VendorPrefix, Box<[Selector<'i, Impl>]>),
  /// The `:has` pseudo-class.
  ///
  /// https://www.w3.org/TR/selectors/#relational
  Has(Box<[Selector<'i, Impl>]>),
  /// An implementation-dependent pseudo-element selector.
  PseudoElement(Impl::PseudoElement),
  /// A nesting selector:
  ///
  /// https://drafts.csswg.org/css-nesting-1/#nest-selector
  ///
  /// NOTE: This is a lightningcss addition.
  Nesting,
}

#[cfg(feature = "into_owned")]
impl<'any, 'i, Impl: SelectorImpl<'i>, NewSel> static_self::IntoOwned<'any> for Component<'i, Impl>
where
  Impl: static_self::IntoOwned<'any, Owned = NewSel>,
  NewSel: SelectorImpl<'any>,
  Impl::NamespaceUrl: static_self::IntoOwned<'any, Owned = NewSel::NamespaceUrl>,
  Impl::NamespacePrefix: static_self::IntoOwned<'any, Owned = NewSel::NamespacePrefix>,
  Impl::Identifier: static_self::IntoOwned<'any, Owned = NewSel::Identifier>,
  Impl::LocalName: static_self::IntoOwned<'any, Owned = NewSel::LocalName>,
  Impl::AttrValue: static_self::IntoOwned<'any, Owned = NewSel::AttrValue>,
  Impl::NonTSPseudoClass: static_self::IntoOwned<'any, Owned = NewSel::NonTSPseudoClass>,
  Impl::PseudoElement: static_self::IntoOwned<'any, Owned = NewSel::PseudoElement>,
  Impl::VendorPrefix: static_self::IntoOwned<'any, Owned = NewSel::VendorPrefix>,
{
  type Owned = Component<'any, NewSel>;

  fn into_owned(self) -> Self::Owned {
    match self {
      Component::Combinator(c) => Component::Combinator(c.into_owned()),
      Component::ExplicitAnyNamespace => Component::ExplicitAnyNamespace,
      Component::ExplicitNoNamespace => Component::ExplicitNoNamespace,
      Component::DefaultNamespace(c) => Component::DefaultNamespace(c.into_owned()),
      Component::Namespace(a, b) => Component::Namespace(a.into_owned(), b.into_owned()),
      Component::ExplicitUniversalType => Component::ExplicitUniversalType,
      Component::LocalName(c) => Component::LocalName(c.into_owned()),
      Component::ID(c) => Component::ID(c.into_owned()),
      Component::Class(c) => Component::Class(c.into_owned()),
      Component::AttributeInNoNamespaceExists {
        local_name,
        local_name_lower,
      } => Component::AttributeInNoNamespaceExists {
        local_name: local_name.into_owned(),
        local_name_lower: local_name_lower.into_owned(),
      },
      Component::AttributeInNoNamespace {
        local_name,
        operator,
        value,
        case_sensitivity,
        never_matches,
      } => {
        let value = value.into_owned();
        Component::AttributeInNoNamespace {
          local_name: local_name.into_owned(),
          operator,
          value,
          case_sensitivity,
          never_matches,
        }
      }
      Component::AttributeOther(c) => Component::AttributeOther(c.into_owned()),
      Component::Negation(c) => Component::Negation(c.into_owned()),
      Component::Root => Component::Root,
      Component::Empty => Component::Empty,
      Component::Scope => Component::Scope,
      Component::Nth(c) => Component::Nth(c.into_owned()),
      Component::NthOf(c) => Component::NthOf(c.into_owned()),
      Component::NonTSPseudoClass(c) => Component::NonTSPseudoClass(c.into_owned()),
      Component::Slotted(c) => Component::Slotted(c.into_owned()),
      Component::Part(c) => Component::Part(c.into_owned()),
      Component::Host(c) => Component::Host(c.into_owned()),
      Component::Where(c) => Component::Where(c.into_owned()),
      Component::Is(c) => Component::Is(c.into_owned()),
      Component::Any(a, b) => Component::Any(a.into_owned(), b.into_owned()),
      Component::Has(c) => Component::Has(c.into_owned()),
      Component::PseudoElement(c) => Component::PseudoElement(c.into_owned()),
      Component::Nesting => Component::Nesting,
    }
  }
}

impl<'i, Impl: SelectorImpl<'i>> Component<'i, Impl> {
  /// Returns true if this is a combinator.
  pub fn is_combinator(&self) -> bool {
    matches!(*self, Component::Combinator(_))
  }

  /// Returns the value as a combinator if applicable, None otherwise.
  pub fn as_combinator(&self) -> Option<Combinator> {
    match *self {
      Component::Combinator(c) => Some(c),
      _ => None,
    }
  }

  /// Whether this component is valid after a pseudo-element. Only intended
  /// for sanity-checking.
  pub fn maybe_allowed_after_pseudo_element(&self) -> bool {
    match *self {
      Component::NonTSPseudoClass(..) => true,
      Component::Negation(ref selectors) | Component::Is(ref selectors) | Component::Where(ref selectors) => {
        selectors
          .iter()
          .all(|selector| selector.iter_raw_match_order().all(|c| c.maybe_allowed_after_pseudo_element()))
      }
      _ => false,
    }
  }

  /// Whether a given selector should match for stateless pseudo-elements.
  ///
  /// This is a bit subtle: Only selectors that return true in
  /// `maybe_allowed_after_pseudo_element` should end up here, and
  /// `NonTSPseudoClass` never matches (as it is a stateless pseudo after
  /// all).
  fn matches_for_stateless_pseudo_element(&self) -> bool {
    debug_assert!(
      self.maybe_allowed_after_pseudo_element(),
      "Someone messed up pseudo-element parsing: {:?}",
      *self
    );
    match *self {
      Component::Negation(ref selectors) => !selectors.iter().all(|selector| {
        selector
          .iter_raw_match_order()
          .all(|c| c.matches_for_stateless_pseudo_element())
      }),
      Component::Is(ref selectors) | Component::Where(ref selectors) => selectors.iter().any(|selector| {
        selector
          .iter_raw_match_order()
          .all(|c| c.matches_for_stateless_pseudo_element())
      }),
      _ => false,
    }
  }

  pub fn visit<V>(&self, visitor: &mut V) -> bool
  where
    V: SelectorVisitor<'i, Impl = Impl>,
  {
    use self::Component::*;
    if !visitor.visit_simple_selector(self) {
      return false;
    }

    match *self {
      Slotted(ref selector) => {
        if !selector.visit(visitor) {
          return false;
        }
      }
      Host(Some(ref selector)) => {
        if !selector.visit(visitor) {
          return false;
        }
      }
      AttributeInNoNamespaceExists {
        ref local_name,
        ref local_name_lower,
      } => {
        if !visitor.visit_attribute_selector(
          &NamespaceConstraint::Specific(&namespace_empty_string::<Impl>()),
          local_name,
          local_name_lower,
        ) {
          return false;
        }
      }
      AttributeInNoNamespace {
        ref local_name,
        never_matches,
        ..
      } if !never_matches => {
        if !visitor.visit_attribute_selector(
          &NamespaceConstraint::Specific(&namespace_empty_string::<Impl>()),
          local_name,
          local_name,
        ) {
          return false;
        }
      }
      AttributeOther(ref attr_selector) if !attr_selector.never_matches => {
        let empty_string;
        let namespace = match attr_selector.namespace() {
          Some(ns) => ns,
          None => {
            empty_string = crate::parser::namespace_empty_string::<Impl>();
            NamespaceConstraint::Specific(&empty_string)
          }
        };
        if !visitor.visit_attribute_selector(
          &namespace,
          &attr_selector.local_name,
          &attr_selector.local_name_lower,
        ) {
          return false;
        }
      }

      NonTSPseudoClass(ref pseudo_class) => {
        if !pseudo_class.visit(visitor) {
          return false;
        }
      }

      Negation(ref list) | Is(ref list) | Where(ref list) => {
        if !visitor.visit_selector_list(&list) {
          return false;
        }
      }
      NthOf(ref nth_of_data) => {
        if !visitor.visit_selector_list(nth_of_data.selectors()) {
          return false;
        }
      }
      _ => {}
    }

    true
  }
}

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct LocalName<'i, Impl: SelectorImpl<'i>> {
  pub name: Impl::LocalName,
  pub lower_name: Impl::LocalName,
}

#[cfg(feature = "into_owned")]
impl<'any, 'i, Impl: SelectorImpl<'i>, NewSel> static_self::IntoOwned<'any> for LocalName<'i, Impl>
where
  Impl: static_self::IntoOwned<'any, Owned = NewSel>,
  NewSel: SelectorImpl<'any>,
  Impl::LocalName: static_self::IntoOwned<'any, Owned = NewSel::LocalName>,
{
  type Owned = LocalName<'any, NewSel>;

  fn into_owned(self) -> Self::Owned {
    LocalName {
      name: self.name.into_owned(),
      lower_name: self.lower_name.into_owned(),
    }
  }
}

impl<'i, Impl: SelectorImpl<'i>> Debug for Selector<'i, Impl> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.write_str("Selector(")?;
    self.to_css(f)?;
    write!(f, ", specificity = 0x{:x})", self.specificity())
  }
}

impl<'i, Impl: SelectorImpl<'i>> Debug for Component<'i, Impl> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    self.to_css(f)
  }
}
impl<'i, Impl: SelectorImpl<'i>> Debug for AttrSelectorWithOptionalNamespace<'i, Impl> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    self.to_css(f)
  }
}
impl<'i, Impl: SelectorImpl<'i>> Debug for LocalName<'i, Impl> {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    self.to_css(f)
  }
}

#[cfg(feature = "serde")]
impl<'i, Impl: SelectorImpl<'i>> serde::Serialize for LocalName<'i, Impl>
where
  Impl::LocalName: serde::Serialize,
{
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    self.name.serialize(serializer)
  }
}

#[cfg(feature = "serde")]
impl<'i, 'de: 'i, Impl: SelectorImpl<'i>> serde::Deserialize<'de> for LocalName<'i, Impl>
where
  Impl::LocalName: serde::Deserialize<'de>,
{
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    let name = Impl::LocalName::deserialize(deserializer)?;
    let lower_name = to_ascii_lowercase(name.as_ref().to_string().into()).into();
    Ok(LocalName { name, lower_name })
  }
}

fn serialize_selector_list<'a, 'i: 'a, Impl, I, W>(iter: I, dest: &mut W) -> fmt::Result
where
  Impl: SelectorImpl<'i>,
  I: Iterator<Item = &'a Selector<'i, Impl>>,
  W: fmt::Write,
{
  let mut first = true;
  for selector in iter {
    if !first {
      dest.write_str(", ")?;
    }
    first = false;
    selector.to_css(dest)?;
  }
  Ok(())
}

impl<'i, Impl: SelectorImpl<'i>> ToCss for SelectorList<'i, Impl> {
  fn to_css<W>(&self, dest: &mut W) -> fmt::Result
  where
    W: fmt::Write,
  {
    serialize_selector_list(self.0.iter(), dest)
  }
}

impl<'i, Impl: SelectorImpl<'i>> fmt::Display for SelectorList<'i, Impl> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
    Impl::to_css(self, f)
  }
}

impl<'i, Impl: SelectorImpl<'i>> ToCss for Selector<'i, Impl> {
  fn to_css<W>(&self, dest: &mut W) -> fmt::Result
  where
    W: fmt::Write,
  {
    // Compound selectors invert the order of their contents, so we need to
    // undo that during serialization.
    //
    // This two-iterator strategy involves walking over the selector twice.
    // We could do something more clever, but selector serialization probably
    // isn't hot enough to justify it, and the stringification likely
    // dominates anyway.
    //
    // NB: A parse-order iterator is a Rev<>, which doesn't expose as_slice(),
    // which we need for |split|. So we split by combinators on a match-order
    // sequence and then reverse.

    let mut combinators = self.iter_raw_match_order().rev().filter_map(|x| x.as_combinator());
    let compound_selectors = self.iter_raw_match_order().as_slice().split(|x| x.is_combinator()).rev();

    let mut combinators_exhausted = false;
    for compound in compound_selectors {
      debug_assert!(!combinators_exhausted);

      // https://drafts.csswg.org/cssom/#serializing-selectors
      if compound.is_empty() {
        continue;
      }

      // 1. If there is only one simple selector in the compound selectors
      //    which is a universal selector, append the result of
      //    serializing the universal selector to s.
      //
      // Check if `!compound.empty()` first--this can happen if we have
      // something like `... > ::before`, because we store `>` and `::`
      // both as combinators internally.
      //
      // If we are in this case, after we have serialized the universal
      // selector, we skip Step 2 and continue with the algorithm.
      let (can_elide_namespace, first_non_namespace) = match compound[0] {
        Component::ExplicitAnyNamespace | Component::ExplicitNoNamespace | Component::Namespace(..) => (false, 1),
        Component::DefaultNamespace(..) => (true, 1),
        _ => (true, 0),
      };
      let mut perform_step_2 = true;
      let next_combinator = combinators.next();
      if first_non_namespace == compound.len() - 1 {
        match (next_combinator, &compound[first_non_namespace]) {
          // We have to be careful here, because if there is a
          // pseudo element "combinator" there isn't really just
          // the one simple selector. Technically this compound
          // selector contains the pseudo element selector as well
          // -- Combinator::PseudoElement, just like
          // Combinator::SlotAssignment, don't exist in the
          // spec.
          (Some(Combinator::PseudoElement), _) | (Some(Combinator::SlotAssignment), _) => (),
          (_, &Component::ExplicitUniversalType) => {
            // Iterate over everything so we serialize the namespace
            // too.
            for simple in compound.iter() {
              simple.to_css(dest)?;
            }
            // Skip step 2, which is an "otherwise".
            perform_step_2 = false;
          }
          _ => (),
        }
      }

      // 2. Otherwise, for each simple selector in the compound selectors
      //    that is not a universal selector of which the namespace prefix
      //    maps to a namespace that is not the default namespace
      //    serialize the simple selector and append the result to s.
      //
      // See https://github.com/w3c/csswg-drafts/issues/1606, which is
      // proposing to change this to match up with the behavior asserted
      // in cssom/serialize-namespaced-type-selectors.html, which the
      // following code tries to match.
      if perform_step_2 {
        for simple in compound.iter() {
          if let Component::ExplicitUniversalType = *simple {
            // Can't have a namespace followed by a pseudo-element
            // selector followed by a universal selector in the same
            // compound selector, so we don't have to worry about the
            // real namespace being in a different `compound`.
            if can_elide_namespace {
              continue;
            }
          }
          simple.to_css(dest)?;
        }
      }

      // 3. If this is not the last part of the chain of the selector
      //    append a single SPACE (U+0020), followed by the combinator
      //    ">", "+", "~", ">>", "||", as appropriate, followed by another
      //    single SPACE (U+0020) if the combinator was not whitespace, to
      //    s.
      match next_combinator {
        Some(c) => c.to_css(dest)?,
        None => combinators_exhausted = true,
      };

      // 4. If this is the last part of the chain of the selector and
      //    there is a pseudo-element, append "::" followed by the name of
      //    the pseudo-element, to s.
      //
      // (we handle this above)
    }

    Ok(())
  }
}

impl ToCss for Combinator {
  fn to_css<W>(&self, dest: &mut W) -> fmt::Result
  where
    W: fmt::Write,
  {
    match *self {
      Combinator::Child => dest.write_str(" > "),
      Combinator::Descendant => dest.write_str(" "),
      Combinator::NextSibling => dest.write_str(" + "),
      Combinator::LaterSibling => dest.write_str(" ~ "),
      Combinator::DeepDescendant => dest.write_str(" >>> "),
      Combinator::Deep => dest.write_str(" /deep/ "),
      Combinator::PseudoElement | Combinator::Part | Combinator::SlotAssignment => Ok(()),
    }
  }
}

impl<'i, Impl: SelectorImpl<'i>> ToCss for Component<'i, Impl> {
  fn to_css<W>(&self, dest: &mut W) -> fmt::Result
  where
    W: fmt::Write,
  {
    use self::Component::*;

    match *self {
      Combinator(ref c) => c.to_css(dest),
      Slotted(ref selector) => {
        dest.write_str("::slotted(")?;
        selector.to_css(dest)?;
        dest.write_char(')')
      }
      Part(ref part_names) => {
        dest.write_str("::part(")?;
        for (i, name) in part_names.iter().enumerate() {
          if i != 0 {
            dest.write_char(' ')?;
          }
          name.to_css(dest)?;
        }
        dest.write_char(')')
      }
      PseudoElement(ref p) => p.to_css(dest),
      ID(ref s) => {
        dest.write_char('#')?;
        s.to_css(dest)
      }
      Class(ref s) => {
        dest.write_char('.')?;
        s.to_css(dest)
      }
      LocalName(ref s) => s.to_css(dest),
      ExplicitUniversalType => dest.write_char('*'),

      DefaultNamespace(_) => Ok(()),
      ExplicitNoNamespace => dest.write_char('|'),
      ExplicitAnyNamespace => dest.write_str("*|"),
      Namespace(ref prefix, _) => {
        prefix.to_css(dest)?;
        dest.write_char('|')
      }

      AttributeInNoNamespaceExists { ref local_name, .. } => {
        dest.write_char('[')?;
        local_name.to_css(dest)?;
        dest.write_char(']')
      }
      AttributeInNoNamespace {
        ref local_name,
        operator,
        ref value,
        case_sensitivity,
        ..
      } => {
        dest.write_char('[')?;
        local_name.to_css(dest)?;
        operator.to_css(dest)?;
        value.to_css(dest)?;
        match case_sensitivity {
          ParsedCaseSensitivity::CaseSensitive
          | ParsedCaseSensitivity::AsciiCaseInsensitiveIfInHtmlElementInHtmlDocument => {}
          ParsedCaseSensitivity::AsciiCaseInsensitive => dest.write_str(" i")?,
          ParsedCaseSensitivity::ExplicitCaseSensitive => dest.write_str(" s")?,
        }
        dest.write_char(']')
      }
      AttributeOther(ref attr_selector) => attr_selector.to_css(dest),

      // Pseudo-classes
      Root => dest.write_str(":root"),
      Empty => dest.write_str(":empty"),
      Scope => dest.write_str(":scope"),
      Host(ref selector) => {
        dest.write_str(":host")?;
        if let Some(ref selector) = *selector {
          dest.write_char('(')?;
          selector.to_css(dest)?;
          dest.write_char(')')?;
        }
        Ok(())
      }
      Nth(ref nth_data) => {
        nth_data.write_start(dest, nth_data.is_function())?;
        if nth_data.is_function() {
          nth_data.write_affine(dest)?;
          dest.write_char(')')?;
        }
        Ok(())
      }
      NthOf(ref nth_of_data) => {
        let nth_data = nth_of_data.nth_data();
        nth_data.write_start(dest, true)?;
        debug_assert!(
          nth_data.is_function,
          "A selector must be a function to hold An+B notation"
        );
        nth_data.write_affine(dest)?;
        debug_assert!(
          matches!(nth_data.ty, NthType::Child | NthType::LastChild),
          "Only :nth-child or :nth-last-child can be of a selector list"
        );
        debug_assert!(
          !nth_of_data.selectors().is_empty(),
          "The selector list should not be empty"
        );
        dest.write_str(" of ")?;
        serialize_selector_list(nth_of_data.selectors().iter(), dest)?;
        dest.write_char(')')
      }
      Is(ref list) | Where(ref list) | Negation(ref list) | Has(ref list) | Any(_, ref list) => {
        match *self {
          Where(..) => dest.write_str(":where(")?,
          Is(..) => dest.write_str(":is(")?,
          Negation(..) => dest.write_str(":not(")?,
          Has(..) => dest.write_str(":has(")?,
          Any(ref prefix, _) => {
            dest.write_char(':')?;
            prefix.to_css(dest)?;
            dest.write_str("any(")?;
          }
          _ => unreachable!(),
        }
        serialize_selector_list(list.iter(), dest)?;
        dest.write_str(")")
      }
      NonTSPseudoClass(ref pseudo) => pseudo.to_css(dest),
      Nesting => dest.write_char('&'),
    }
  }
}

impl<'i, Impl: SelectorImpl<'i>> ToCss for AttrSelectorWithOptionalNamespace<'i, Impl> {
  fn to_css<W>(&self, dest: &mut W) -> fmt::Result
  where
    W: fmt::Write,
  {
    dest.write_char('[')?;
    match self.namespace {
      Some(NamespaceConstraint::Specific((ref prefix, _))) => {
        prefix.to_css(dest)?;
        dest.write_char('|')?
      }
      Some(NamespaceConstraint::Any) => dest.write_str("*|")?,
      None => {}
    }
    self.local_name.to_css(dest)?;
    match self.operation {
      ParsedAttrSelectorOperation::Exists => {}
      ParsedAttrSelectorOperation::WithValue {
        operator,
        case_sensitivity,
        ref expected_value,
      } => {
        operator.to_css(dest)?;
        expected_value.to_css(dest)?;
        match case_sensitivity {
          ParsedCaseSensitivity::CaseSensitive
          | ParsedCaseSensitivity::AsciiCaseInsensitiveIfInHtmlElementInHtmlDocument => {}
          ParsedCaseSensitivity::AsciiCaseInsensitive => dest.write_str(" i")?,
          ParsedCaseSensitivity::ExplicitCaseSensitive => dest.write_str(" s")?,
        }
      }
    }
    dest.write_char(']')
  }
}

impl<'i, Impl: SelectorImpl<'i>> ToCss for LocalName<'i, Impl> {
  fn to_css<W>(&self, dest: &mut W) -> fmt::Result
  where
    W: fmt::Write,
  {
    self.name.to_css(dest)
  }
}

/// Build up a Selector.
/// selector : simple_selector_sequence [ combinator simple_selector_sequence ]* ;
///
/// `Err` means invalid selector.
fn parse_selector<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  state: &mut SelectorParsingState,
  nesting_requirement: NestingRequirement,
) -> Result<Selector<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  if nesting_requirement == NestingRequirement::Prefixed {
    let state = input.state();
    if !input.expect_delim('&').is_ok() {
      return Err(input.new_custom_error(SelectorParseErrorKind::MissingNestingPrefix));
    }
    input.reset(&state);
  }

  let mut builder = SelectorBuilder::default();

  'outer_loop: loop {
    // Parse a sequence of simple selectors.
    let empty = parse_compound_selector(parser, state, input, &mut builder)?;
    if empty {
      return Err(input.new_custom_error(if builder.has_combinators() {
        SelectorParseErrorKind::DanglingCombinator
      } else {
        SelectorParseErrorKind::EmptySelector
      }));
    }

    if state.intersects(SelectorParsingState::AFTER_PSEUDO) {
      // Input should be exhausted here.
      let source_location = input.current_source_location();
      if let Ok(next) = input.next() {
        let next = next.clone();
        return Err(
          source_location.new_custom_error(SelectorParseErrorKind::UnexpectedSelectorAfterPseudoElement(next)),
        );
      }
      break;
    }

    // Parse a combinator.
    let combinator;
    let mut any_whitespace = false;
    loop {
      let before_this_token = input.state();
      match input.next_including_whitespace() {
        Err(_e) => break 'outer_loop,
        Ok(&Token::WhiteSpace(_)) => any_whitespace = true,
        Ok(&Token::Delim('>')) => {
          if parser.deep_combinator_enabled()
            && input
              .try_parse(|input| {
                input.expect_delim('>')?;
                input.expect_delim('>')
              })
              .is_ok()
          {
            combinator = Combinator::DeepDescendant;
          } else {
            combinator = Combinator::Child;
          }
          break;
        }
        Ok(&Token::Delim('+')) => {
          combinator = Combinator::NextSibling;
          break;
        }
        Ok(&Token::Delim('~')) => {
          combinator = Combinator::LaterSibling;
          break;
        }
        Ok(&Token::Delim('/')) if parser.deep_combinator_enabled() => {
          if input
            .try_parse(|input| {
              input.expect_ident_matching("deep")?;
              input.expect_delim('/')
            })
            .is_ok()
          {
            combinator = Combinator::Deep;
            break;
          } else {
            break 'outer_loop;
          }
        }
        Ok(_) => {
          input.reset(&before_this_token);
          if any_whitespace {
            combinator = Combinator::Descendant;
            break;
          } else {
            break 'outer_loop;
          }
        }
      }
    }

    if !state.allows_combinators() {
      return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
    }

    builder.push_combinator(combinator);
  }

  if !state.contains(SelectorParsingState::AFTER_NESTING) {
    match nesting_requirement {
      NestingRequirement::Implicit => {
        builder.add_nesting_prefix();
      }
      NestingRequirement::Contained | NestingRequirement::Prefixed => {
        return Err(input.new_custom_error(SelectorParseErrorKind::MissingNestingSelector));
      }
      _ => {}
    }
  }

  let has_pseudo_element = state
    .intersects(SelectorParsingState::AFTER_PSEUDO_ELEMENT | SelectorParsingState::AFTER_UNKNOWN_PSEUDO_ELEMENT);
  let slotted = state.intersects(SelectorParsingState::AFTER_SLOTTED);
  let part = state.intersects(SelectorParsingState::AFTER_PART);
  let (spec, components) = builder.build(has_pseudo_element, slotted, part);
  Ok(Selector(spec, components))
}

impl<'i, Impl: SelectorImpl<'i>> Selector<'i, Impl> {
  /// Parse a selector, without any pseudo-element.
  #[inline]
  pub fn parse<'t, P>(parser: &P, input: &mut CssParser<'i, 't>) -> Result<Self, ParseError<'i, P::Error>>
  where
    P: Parser<'i, Impl = Impl>,
  {
    parse_selector(
      parser,
      input,
      &mut SelectorParsingState::empty(),
      NestingRequirement::None,
    )
  }
}

fn parse_relative_selector<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  state: &mut SelectorParsingState,
  mut nesting_requirement: NestingRequirement,
) -> Result<Selector<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  // https://www.w3.org/TR/selectors-4/#parse-relative-selector
  let s = input.state();
  let combinator = match input.next()? {
    Token::Delim('>') => Some(Combinator::Child),
    Token::Delim('+') => Some(Combinator::NextSibling),
    Token::Delim('~') => Some(Combinator::LaterSibling),
    _ => {
      input.reset(&s);
      None
    }
  };

  let scope = if nesting_requirement == NestingRequirement::Implicit {
    Component::Nesting
  } else {
    Component::Scope
  };

  if combinator.is_some() {
    nesting_requirement = NestingRequirement::None;
  }

  let mut selector = parse_selector(parser, input, state, nesting_requirement)?;
  if let Some(combinator) = combinator {
    // https://www.w3.org/TR/selectors/#absolutizing
    selector.1.push(Component::Combinator(combinator));
    selector.1.push(scope);
  }

  Ok(selector)
}

/// * `Err(())`: Invalid selector, abort
/// * `Ok(false)`: Not a type selector, could be something else. `input` was not consumed.
/// * `Ok(true)`: Length 0 (`*|*`), 1 (`*|E` or `ns|*`) or 2 (`|E` or `ns|E`)
fn parse_type_selector<'i, 't, P, Impl, S>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  state: SelectorParsingState,
  sink: &mut S,
) -> Result<bool, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
  S: Push<Component<'i, Impl>>,
{
  match parse_qualified_name(parser, input, /* in_attr_selector = */ false) {
    Err(ParseError {
      kind: ParseErrorKind::Basic(BasicParseErrorKind::EndOfInput),
      ..
    })
    | Ok(OptionalQName::None(_)) => Ok(false),
    Ok(OptionalQName::Some(namespace, local_name)) => {
      if state.intersects(SelectorParsingState::AFTER_PSEUDO) {
        return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
      }
      match namespace {
        QNamePrefix::ImplicitAnyNamespace => {}
        QNamePrefix::ImplicitDefaultNamespace(url) => sink.push(Component::DefaultNamespace(url)),
        QNamePrefix::ExplicitNamespace(prefix, url) => sink.push(match parser.default_namespace() {
          Some(ref default_url) if url == *default_url => Component::DefaultNamespace(url),
          _ => Component::Namespace(prefix, url),
        }),
        QNamePrefix::ExplicitNoNamespace => sink.push(Component::ExplicitNoNamespace),
        QNamePrefix::ExplicitAnyNamespace => {
          // Element type selectors that have no namespace
          // component (no namespace separator) represent elements
          // without regard to the element's namespace (equivalent
          // to "*|") unless a default namespace has been declared
          // for namespaced selectors (e.g. in CSS, in the style
          // sheet). If a default namespace has been declared,
          // such selectors will represent only elements in the
          // default namespace.
          // -- Selectors § 6.1.1
          // So we'll have this act the same as the
          // QNamePrefix::ImplicitAnyNamespace case.
          // For lightning css this logic was removed, should be handled when matching.
          sink.push(Component::ExplicitAnyNamespace)
        }
        QNamePrefix::ImplicitNoNamespace => {
          unreachable!() // Not returned with in_attr_selector = false
        }
      }
      match local_name {
        Some(name) => sink.push(Component::LocalName(LocalName {
          lower_name: to_ascii_lowercase(name.clone()).into(),
          name: name.into(),
        })),
        None => sink.push(Component::ExplicitUniversalType),
      }
      Ok(true)
    }
    Err(e) => Err(e),
  }
}

#[derive(Debug)]
enum SimpleSelectorParseResult<'i, Impl: SelectorImpl<'i>> {
  SimpleSelector(Component<'i, Impl>),
  PseudoElement(Impl::PseudoElement),
  SlottedPseudo(Selector<'i, Impl>),
  PartPseudo(Box<[Impl::Identifier]>),
}

#[derive(Debug)]
enum QNamePrefix<'i, Impl: SelectorImpl<'i>> {
  ImplicitNoNamespace,                                          // `foo` in attr selectors
  ImplicitAnyNamespace,                                         // `foo` in type selectors, without a default ns
  ImplicitDefaultNamespace(Impl::NamespaceUrl),                 // `foo` in type selectors, with a default ns
  ExplicitNoNamespace,                                          // `|foo`
  ExplicitAnyNamespace,                                         // `*|foo`
  ExplicitNamespace(Impl::NamespacePrefix, Impl::NamespaceUrl), // `prefix|foo`
}

enum OptionalQName<'i, Impl: SelectorImpl<'i>> {
  Some(QNamePrefix<'i, Impl>, Option<CowRcStr<'i>>),
  None(Token<'i>),
}

/// * `Err(())`: Invalid selector, abort
/// * `Ok(None(token))`: Not a simple selector, could be something else. `input` was not consumed,
///                      but the token is still returned.
/// * `Ok(Some(namespace, local_name))`: `None` for the local name means a `*` universal selector
fn parse_qualified_name<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  in_attr_selector: bool,
) -> Result<OptionalQName<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  let default_namespace = |local_name| {
    let namespace = match parser.default_namespace() {
      Some(url) => QNamePrefix::ImplicitDefaultNamespace(url),
      None => QNamePrefix::ImplicitAnyNamespace,
    };
    Ok(OptionalQName::Some(namespace, local_name))
  };

  let explicit_namespace = |input: &mut CssParser<'i, 't>, namespace| {
    let location = input.current_source_location();
    match input.next_including_whitespace() {
      Ok(&Token::Delim('*')) if !in_attr_selector => Ok(OptionalQName::Some(namespace, None)),
      Ok(&Token::Ident(ref local_name)) => Ok(OptionalQName::Some(namespace, Some(local_name.clone()))),
      Ok(t) if in_attr_selector => {
        let e = SelectorParseErrorKind::InvalidQualNameInAttr(t.clone());
        Err(location.new_custom_error(e))
      }
      Ok(t) => Err(location.new_custom_error(SelectorParseErrorKind::ExplicitNamespaceUnexpectedToken(t.clone()))),
      Err(e) => Err(e.into()),
    }
  };

  let start = input.state();
  // FIXME: remove clone() when lifetimes are non-lexical
  match input.next_including_whitespace().map(|t| t.clone()) {
    Ok(Token::Ident(value)) => {
      let after_ident = input.state();
      match input.next_including_whitespace() {
        Ok(&Token::Delim('|')) => {
          let prefix = value.clone().into();
          let result = parser.namespace_for_prefix(&prefix);
          let url = result.ok_or(
            after_ident
              .source_location()
              .new_custom_error(SelectorParseErrorKind::ExpectedNamespace(value)),
          )?;
          explicit_namespace(input, QNamePrefix::ExplicitNamespace(prefix, url))
        }
        _ => {
          input.reset(&after_ident);
          if in_attr_selector {
            Ok(OptionalQName::Some(QNamePrefix::ImplicitNoNamespace, Some(value)))
          } else {
            default_namespace(Some(value))
          }
        }
      }
    }
    Ok(Token::Delim('*')) => {
      let after_star = input.state();
      // FIXME: remove clone() when lifetimes are non-lexical
      match input.next_including_whitespace().map(|t| t.clone()) {
        Ok(Token::Delim('|')) => explicit_namespace(input, QNamePrefix::ExplicitAnyNamespace),
        result => {
          input.reset(&after_star);
          if in_attr_selector {
            match result {
              Ok(t) => Err(
                after_star
                  .source_location()
                  .new_custom_error(SelectorParseErrorKind::ExpectedBarInAttr(t)),
              ),
              Err(e) => Err(e.into()),
            }
          } else {
            default_namespace(None)
          }
        }
      }
    }
    Ok(Token::Delim('|')) => explicit_namespace(input, QNamePrefix::ExplicitNoNamespace),
    Ok(t) => {
      input.reset(&start);
      Ok(OptionalQName::None(t))
    }
    Err(e) => {
      input.reset(&start);
      Err(e.into())
    }
  }
}

fn parse_attribute_selector<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
) -> Result<Component<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  let namespace;
  let local_name;

  input.skip_whitespace();

  match parse_qualified_name(parser, input, /* in_attr_selector = */ true)? {
    OptionalQName::None(t) => {
      return Err(input.new_custom_error(SelectorParseErrorKind::NoQualifiedNameInAttributeSelector(t)));
    }
    OptionalQName::Some(_, None) => unreachable!(),
    OptionalQName::Some(ns, Some(ln)) => {
      local_name = ln;
      namespace = match ns {
        QNamePrefix::ImplicitNoNamespace | QNamePrefix::ExplicitNoNamespace => None,
        QNamePrefix::ExplicitNamespace(prefix, url) => Some(NamespaceConstraint::Specific((prefix, url))),
        QNamePrefix::ExplicitAnyNamespace => Some(NamespaceConstraint::Any),
        QNamePrefix::ImplicitAnyNamespace | QNamePrefix::ImplicitDefaultNamespace(_) => {
          unreachable!() // Not returned with in_attr_selector = true
        }
      }
    }
  }

  let location = input.current_source_location();
  let operator = match input.next() {
    // [foo]
    Err(_) => {
      let local_name_lower = to_ascii_lowercase(local_name.clone()).into();
      let local_name = local_name.into();
      if let Some(namespace) = namespace {
        return Ok(Component::AttributeOther(Box::new(AttrSelectorWithOptionalNamespace {
          namespace: Some(namespace),
          local_name,
          local_name_lower,
          operation: ParsedAttrSelectorOperation::Exists,
          never_matches: false,
        })));
      } else {
        return Ok(Component::AttributeInNoNamespaceExists {
          local_name,
          local_name_lower,
        });
      }
    }

    // [foo=bar]
    Ok(&Token::Delim('=')) => AttrSelectorOperator::Equal,
    // [foo~=bar]
    Ok(&Token::IncludeMatch) => AttrSelectorOperator::Includes,
    // [foo|=bar]
    Ok(&Token::DashMatch) => AttrSelectorOperator::DashMatch,
    // [foo^=bar]
    Ok(&Token::PrefixMatch) => AttrSelectorOperator::Prefix,
    // [foo*=bar]
    Ok(&Token::SubstringMatch) => AttrSelectorOperator::Substring,
    // [foo$=bar]
    Ok(&Token::SuffixMatch) => AttrSelectorOperator::Suffix,
    Ok(t) => {
      return Err(
        location.new_custom_error(SelectorParseErrorKind::UnexpectedTokenInAttributeSelector(t.clone())),
      );
    }
  };

  let value = match input.expect_ident_or_string() {
    Ok(t) => t.clone(),
    Err(BasicParseError {
      kind: BasicParseErrorKind::UnexpectedToken(t),
      location,
    }) => return Err(location.new_custom_error(SelectorParseErrorKind::BadValueInAttr(t))),
    Err(e) => return Err(e.into()),
  };
  let never_matches = match operator {
    AttrSelectorOperator::Equal | AttrSelectorOperator::DashMatch => false,

    AttrSelectorOperator::Includes => value.is_empty() || value.contains(SELECTOR_WHITESPACE),

    AttrSelectorOperator::Prefix | AttrSelectorOperator::Substring | AttrSelectorOperator::Suffix => {
      value.is_empty()
    }
  };

  let attribute_flags = parse_attribute_flags(input)?;

  let value = value.into();
  let case_sensitivity;
  // copied from to_ascii_lowercase function, so we can know whether it is already lower case.
  let (local_name_lower_cow, local_name_is_ascii_lowercase) =
    if let Some(first_uppercase) = local_name.bytes().position(|byte| byte >= b'A' && byte <= b'Z') {
      let mut string = local_name.to_string();
      string[first_uppercase..].make_ascii_lowercase();
      (string.into(), false)
    } else {
      (local_name.clone(), true)
    };
  case_sensitivity = attribute_flags.to_case_sensitivity(local_name_lower_cow.as_ref(), namespace.is_some());
  let local_name_lower = local_name_lower_cow.into();
  let local_name = local_name.into();
  if namespace.is_some() || !local_name_is_ascii_lowercase {
    Ok(Component::AttributeOther(Box::new(AttrSelectorWithOptionalNamespace {
      namespace,
      local_name,
      local_name_lower,
      never_matches,
      operation: ParsedAttrSelectorOperation::WithValue {
        operator,
        case_sensitivity,
        expected_value: value,
      },
    })))
  } else {
    Ok(Component::AttributeInNoNamespace {
      local_name,
      operator,
      value,
      case_sensitivity,
      never_matches,
    })
  }
}

/// An attribute selector can have 's' or 'i' as flags, or no flags at all.
enum AttributeFlags {
  // Matching should be case-sensitive ('s' flag).
  CaseSensitive,
  // Matching should be case-insensitive ('i' flag).
  AsciiCaseInsensitive,
  // No flags.  Matching behavior depends on the name of the attribute.
  CaseSensitivityDependsOnName,
}

impl AttributeFlags {
  fn to_case_sensitivity(self, local_name: &str, have_namespace: bool) -> ParsedCaseSensitivity {
    match self {
      AttributeFlags::CaseSensitive => ParsedCaseSensitivity::ExplicitCaseSensitive,
      AttributeFlags::AsciiCaseInsensitive => ParsedCaseSensitivity::AsciiCaseInsensitive,
      AttributeFlags::CaseSensitivityDependsOnName => {
        if !have_namespace
          && include!(concat!(env!("OUT_DIR"), "/ascii_case_insensitive_html_attributes.rs")).contains(local_name)
        {
          ParsedCaseSensitivity::AsciiCaseInsensitiveIfInHtmlElementInHtmlDocument
        } else {
          ParsedCaseSensitivity::CaseSensitive
        }
      }
    }
  }
}

fn parse_attribute_flags<'i, 't>(input: &mut CssParser<'i, 't>) -> Result<AttributeFlags, BasicParseError<'i>> {
  let location = input.current_source_location();
  let token = match input.next() {
    Ok(t) => t,
    Err(..) => {
      // Selectors spec says language-defined; HTML says it depends on the
      // exact attribute name.
      return Ok(AttributeFlags::CaseSensitivityDependsOnName);
    }
  };

  let ident = match *token {
    Token::Ident(ref i) => i,
    ref other => return Err(location.new_basic_unexpected_token_error(other.clone())),
  };

  Ok(match_ignore_ascii_case! {
      ident,
      "i" => AttributeFlags::AsciiCaseInsensitive,
      "s" => AttributeFlags::CaseSensitive,
      _ => return Err(location.new_basic_unexpected_token_error(token.clone())),
  })
}

/// Level 3: Parse **one** simple_selector.  (Though we might insert a second
/// implied "<defaultns>|*" type selector.)
fn parse_negation<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  state: &mut SelectorParsingState,
) -> Result<Component<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  let mut child_state =
    *state | SelectorParsingState::SKIP_DEFAULT_NAMESPACE | SelectorParsingState::DISALLOW_PSEUDOS;
  let list = SelectorList::parse_with_state(
    parser,
    input,
    &mut child_state,
    ParseErrorRecovery::DiscardList,
    NestingRequirement::None,
  )?;

  if child_state.contains(SelectorParsingState::AFTER_NESTING) {
    state.insert(SelectorParsingState::AFTER_NESTING)
  }

  Ok(Component::Negation(list.0.into_vec().into_boxed_slice()))
}

/// simple_selector_sequence
/// : [ type_selector | universal ] [ HASH | class | attrib | pseudo | negation ]*
/// | [ HASH | class | attrib | pseudo | negation ]+
///
/// `Err(())` means invalid selector.
/// `Ok(true)` is an empty selector
fn parse_compound_selector<'i, 't, P, Impl>(
  parser: &P,
  state: &mut SelectorParsingState,
  input: &mut CssParser<'i, 't>,
  builder: &mut SelectorBuilder<'i, Impl>,
) -> Result<bool, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  input.skip_whitespace();

  let mut empty = true;
  if parser.is_nesting_allowed() && input.try_parse(|input| input.expect_delim('&')).is_ok() {
    state.insert(SelectorParsingState::AFTER_NESTING);
    builder.push_simple_selector(Component::Nesting);
    empty = false;
  }

  if parse_type_selector(parser, input, *state, builder)? {
    empty = false;
  }

  loop {
    let result = match parse_one_simple_selector(parser, input, state)? {
      None => break,
      Some(result) => result,
    };

    if empty {
      if let Some(url) = parser.default_namespace() {
        // If there was no explicit type selector, but there is a
        // default namespace, there is an implicit "<defaultns>|*" type
        // selector. Except for :host() or :not() / :is() / :where(),
        // where we ignore it.
        //
        // https://drafts.csswg.org/css-scoping/#host-element-in-tree:
        //
        //     When considered within its own shadow trees, the shadow
        //     host is featureless. Only the :host, :host(), and
        //     :host-context() pseudo-classes are allowed to match it.
        //
        // https://drafts.csswg.org/selectors-4/#featureless:
        //
        //     A featureless element does not match any selector at all,
        //     except those it is explicitly defined to match. If a
        //     given selector is allowed to match a featureless element,
        //     it must do so while ignoring the default namespace.
        //
        // https://drafts.csswg.org/selectors-4/#matches
        //
        //     Default namespace declarations do not affect the compound
        //     selector representing the subject of any selector within
        //     a :is() pseudo-class, unless that compound selector
        //     contains an explicit universal selector or type selector.
        //
        //     (Similar quotes for :where() / :not())
        //
        let ignore_default_ns = state.intersects(SelectorParsingState::SKIP_DEFAULT_NAMESPACE)
          || matches!(result, SimpleSelectorParseResult::SimpleSelector(Component::Host(..)));
        if !ignore_default_ns {
          builder.push_simple_selector(Component::DefaultNamespace(url));
        }
      }
    }

    empty = false;

    match result {
      SimpleSelectorParseResult::SimpleSelector(s) => {
        builder.push_simple_selector(s);
      }
      SimpleSelectorParseResult::PartPseudo(part_names) => {
        state.insert(SelectorParsingState::AFTER_PART);
        builder.push_combinator(Combinator::Part);
        builder.push_simple_selector(Component::Part(part_names));
      }
      SimpleSelectorParseResult::SlottedPseudo(selector) => {
        state.insert(SelectorParsingState::AFTER_SLOTTED);
        builder.push_combinator(Combinator::SlotAssignment);
        builder.push_simple_selector(Component::Slotted(selector));
      }
      SimpleSelectorParseResult::PseudoElement(p) => {
        if !p.is_unknown() {
          state.insert(SelectorParsingState::AFTER_PSEUDO_ELEMENT);
          builder.push_combinator(Combinator::PseudoElement);
        } else {
          state.insert(SelectorParsingState::AFTER_UNKNOWN_PSEUDO_ELEMENT);
        }
        if !p.accepts_state_pseudo_classes() {
          state.insert(SelectorParsingState::AFTER_NON_STATEFUL_PSEUDO_ELEMENT);
        }
        if p.is_webkit_scrollbar() {
          state.insert(SelectorParsingState::AFTER_WEBKIT_SCROLLBAR);
        }
        if p.is_view_transition() {
          state.insert(SelectorParsingState::AFTER_VIEW_TRANSITION);
        }
        builder.push_simple_selector(Component::PseudoElement(p));
      }
    }
  }
  Ok(empty)
}

fn parse_is_or_where<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  state: &mut SelectorParsingState,
  component: impl FnOnce(Box<[Selector<'i, Impl>]>) -> Component<'i, Impl>,
) -> Result<Component<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  debug_assert!(parser.parse_is_and_where());
  // https://drafts.csswg.org/selectors/#matches-pseudo:
  //
  //     Pseudo-elements cannot be represented by the matches-any
  //     pseudo-class; they are not valid within :is().
  //
  let mut child_state =
    *state | SelectorParsingState::SKIP_DEFAULT_NAMESPACE | SelectorParsingState::DISALLOW_PSEUDOS;
  let inner = SelectorList::parse_with_state(
    parser,
    input,
    &mut child_state,
    parser.is_and_where_error_recovery(),
    NestingRequirement::None,
  )?;
  if child_state.contains(SelectorParsingState::AFTER_NESTING) {
    state.insert(SelectorParsingState::AFTER_NESTING)
  }
  Ok(component(inner.0.into_vec().into_boxed_slice()))
}

fn parse_has<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  state: &mut SelectorParsingState,
) -> Result<Component<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  let mut child_state = *state;
  let inner = SelectorList::parse_relative_with_state(
    parser,
    input,
    &mut child_state,
    parser.is_and_where_error_recovery(),
    NestingRequirement::None,
  )?;
  if child_state.contains(SelectorParsingState::AFTER_NESTING) {
    state.insert(SelectorParsingState::AFTER_NESTING)
  }
  Ok(Component::Has(inner.0.into_vec().into_boxed_slice()))
}

fn parse_functional_pseudo_class<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  name: CowRcStr<'i>,
  state: &mut SelectorParsingState,
) -> Result<Component<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  match_ignore_ascii_case! { &name,
      "nth-child" => return parse_nth_pseudo_class(parser, input, *state, NthType::Child),
      "nth-of-type" => return parse_nth_pseudo_class(parser, input, *state, NthType::OfType),
      "nth-last-child" => return parse_nth_pseudo_class(parser, input, *state, NthType::LastChild),
      "nth-last-of-type" => return parse_nth_pseudo_class(parser, input, *state, NthType::LastOfType),
      "nth-col" => return parse_nth_pseudo_class(parser, input, *state, NthType::Col),
      "nth-last-col" => return parse_nth_pseudo_class(parser, input, *state, NthType::LastCol),
      "is" if parser.parse_is_and_where() => return parse_is_or_where(parser, input, state, Component::Is),
      "where" if parser.parse_is_and_where() => return parse_is_or_where(parser, input, state, Component::Where),
      "has" => return parse_has(parser, input, state),
      "host" => {
          if !state.allows_tree_structural_pseudo_classes() {
              return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
          }
          return Ok(Component::Host(Some(parse_inner_compound_selector(parser, input, state)?)));
      },
      "not" => {
          return parse_negation(parser, input, state)
      },
      _ => {}
  }

  if let Some(prefix) = parser.parse_any_prefix(&name) {
    return parse_is_or_where(parser, input, state, |selectors| Component::Any(prefix, selectors));
  }

  if !state.allows_custom_functional_pseudo_classes() {
    return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
  }

  P::parse_non_ts_functional_pseudo_class(parser, name, input).map(Component::NonTSPseudoClass)
}

fn parse_nth_pseudo_class<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  state: SelectorParsingState,
  ty: NthType,
) -> Result<Component<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  if !state.allows_tree_structural_pseudo_classes() {
    return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
  }
  let (a, b) = parse_nth(input)?;
  let nth_data = NthSelectorData {
    ty,
    is_function: true,
    a,
    b,
  };
  if !ty.allows_of_selector() {
    return Ok(Component::Nth(nth_data));
  }

  // Try to parse "of <selector-list>".
  if input.try_parse(|i| i.expect_ident_matching("of")).is_err() {
    return Ok(Component::Nth(nth_data));
  }
  // Whitespace between "of" and the selector list is optional
  // https://github.com/w3c/csswg-drafts/issues/8285
  let mut child_state =
    state | SelectorParsingState::SKIP_DEFAULT_NAMESPACE | SelectorParsingState::DISALLOW_PSEUDOS;
  let selectors = SelectorList::parse_with_state(
    parser,
    input,
    &mut child_state,
    ParseErrorRecovery::IgnoreInvalidSelector,
    NestingRequirement::None,
  )?;
  Ok(Component::NthOf(NthOfSelectorData::new(
    nth_data,
    selectors.0.into_vec().into_boxed_slice(),
  )))
}

/// Returns whether the name corresponds to a CSS2 pseudo-element that
/// can be specified with the single colon syntax (in addition to the
/// double-colon syntax, which can be used for all pseudo-elements).
fn is_css2_pseudo_element(name: &str) -> bool {
  // ** Do not add to this list! **
  match_ignore_ascii_case! { name,
      "before" | "after" | "first-line" | "first-letter" => true,
      _ => false,
  }
}

/// Parse a simple selector other than a type selector.
///
/// * `Err(())`: Invalid selector, abort
/// * `Ok(None)`: Not a simple selector, could be something else. `input` was not consumed.
/// * `Ok(Some(_))`: Parsed a simple selector or pseudo-element
fn parse_one_simple_selector<'i, 't, P, Impl>(
  parser: &P,
  input: &mut CssParser<'i, 't>,
  state: &mut SelectorParsingState,
) -> Result<Option<SimpleSelectorParseResult<'i, Impl>>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  let start = input.state();
  let token_location = input.current_source_location();
  let token = match input.next_including_whitespace().map(|t| t.clone()) {
    Ok(t) => t,
    Err(..) => {
      input.reset(&start);
      return Ok(None);
    }
  };

  Ok(Some(match token {
    Token::IDHash(id) => {
      if state.intersects(SelectorParsingState::AFTER_PSEUDO) {
        return Err(token_location.new_custom_error(
          SelectorParseErrorKind::UnexpectedSelectorAfterPseudoElement(Token::IDHash(id)),
        ));
      }
      let id = Component::ID(id.into());
      SimpleSelectorParseResult::SimpleSelector(id)
    }
    Token::Delim('.') => {
      if state.intersects(SelectorParsingState::AFTER_PSEUDO) {
        return Err(token_location.new_custom_error(
          SelectorParseErrorKind::UnexpectedSelectorAfterPseudoElement(Token::Delim('.')),
        ));
      }
      let location = input.current_source_location();
      let class = match *input.next_including_whitespace()? {
        Token::Ident(ref class) => class.clone(),
        ref t => {
          let e = SelectorParseErrorKind::ClassNeedsIdent(t.clone());
          return Err(location.new_custom_error(e));
        }
      };
      let class = Component::Class(class.into());
      SimpleSelectorParseResult::SimpleSelector(class)
    }
    Token::SquareBracketBlock => {
      if state.intersects(SelectorParsingState::AFTER_PSEUDO) {
        return Err(token_location.new_custom_error(
          SelectorParseErrorKind::UnexpectedSelectorAfterPseudoElement(Token::SquareBracketBlock),
        ));
      }
      let attr = input.parse_nested_block(|input| parse_attribute_selector(parser, input))?;
      SimpleSelectorParseResult::SimpleSelector(attr)
    }
    Token::Colon => {
      let location = input.current_source_location();
      let (is_single_colon, next_token) = match input.next_including_whitespace()?.clone() {
        Token::Colon => (false, input.next_including_whitespace()?.clone()),
        t => (true, t),
      };
      let (name, is_functional) = match next_token {
        Token::Ident(name) => (name, false),
        Token::Function(name) => (name, true),
        t => {
          let e = SelectorParseErrorKind::PseudoElementExpectedIdent(t);
          return Err(input.new_custom_error(e));
        }
      };
      let is_pseudo_element = !is_single_colon || is_css2_pseudo_element(&name);
      if is_pseudo_element {
        if !state.allows_pseudos() {
          return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
        }
        let pseudo_element = if is_functional {
          if P::parse_part(parser) && name.eq_ignore_ascii_case("part") {
            if !state.allows_part() {
              return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
            }
            let names = input.parse_nested_block(|input| {
              let mut result = Vec::with_capacity(1);
              result.push(input.expect_ident_cloned()?.into());
              while !input.is_exhausted() {
                result.push(input.expect_ident_cloned()?.into());
              }
              Ok(result.into_boxed_slice())
            })?;
            return Ok(Some(SimpleSelectorParseResult::PartPseudo(names)));
          }
          if P::parse_slotted(parser) && name.eq_ignore_ascii_case("slotted") {
            if !state.allows_slotted() {
              return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
            }
            let selector =
              input.parse_nested_block(|input| parse_inner_compound_selector(parser, input, state))?;
            return Ok(Some(SimpleSelectorParseResult::SlottedPseudo(selector)));
          }
          input.parse_nested_block(|input| P::parse_functional_pseudo_element(parser, name, input))?
        } else {
          P::parse_pseudo_element(parser, location, name)?
        };

        if state.intersects(SelectorParsingState::AFTER_SLOTTED) && !pseudo_element.valid_after_slotted() {
          return Err(input.new_custom_error(SelectorParseErrorKind::InvalidState));
        }
        SimpleSelectorParseResult::PseudoElement(pseudo_element)
      } else {
        let pseudo_class = if is_functional {
          input.parse_nested_block(|input| parse_functional_pseudo_class(parser, input, name, state))?
        } else {
          parse_simple_pseudo_class(parser, location, name, *state)?
        };
        SimpleSelectorParseResult::SimpleSelector(pseudo_class)
      }
    }
    Token::Delim('&') if parser.is_nesting_allowed() => {
      *state |= SelectorParsingState::AFTER_NESTING;
      SimpleSelectorParseResult::SimpleSelector(Component::Nesting)
    }
    _ => {
      input.reset(&start);
      return Ok(None);
    }
  }))
}

fn parse_simple_pseudo_class<'i, P, Impl>(
  parser: &P,
  location: SourceLocation,
  name: CowRcStr<'i>,
  state: SelectorParsingState,
) -> Result<Component<'i, Impl>, ParseError<'i, P::Error>>
where
  P: Parser<'i, Impl = Impl>,
  Impl: SelectorImpl<'i>,
{
  if !state.allows_non_functional_pseudo_classes() {
    return Err(location.new_custom_error(SelectorParseErrorKind::InvalidState));
  }

  if state.allows_tree_structural_pseudo_classes() {
    match_ignore_ascii_case! { &name,
        "first-child" => return Ok(Component::Nth(NthSelectorData::first(/* of_type = */ false))),
        "last-child" => return Ok(Component::Nth(NthSelectorData::last(/* of_type = */ false))),
        "only-child" => return Ok(Component::Nth(NthSelectorData::only(/* of_type = */ false))),
        "root" => return Ok(Component::Root),
        "empty" => return Ok(Component::Empty),
        "scope" => return Ok(Component::Scope),
        "host" if P::parse_host(parser) => return Ok(Component::Host(None)),
        "first-of-type" => return Ok(Component::Nth(NthSelectorData::first(/* of_type = */ true))),
        "last-of-type" => return Ok(Component::Nth(NthSelectorData::last(/* of_type = */ true))),
        "only-of-type" => return Ok(Component::Nth(NthSelectorData::only(/* of_type = */ true))),
        _ => {},
    }
  }

  // The view-transition pseudo elements accept the :only-child pseudo class.
  // https://w3c.github.io/csswg-drafts/css-view-transitions-1/#pseudo-root
  if state.intersects(SelectorParsingState::AFTER_VIEW_TRANSITION) {
    match_ignore_ascii_case! { &name,
        "only-child" => return Ok(Component::Nth(NthSelectorData::only(/* of_type = */ false))),
        _ => {}
    }
  }

  let pseudo_class = P::parse_non_ts_pseudo_class(parser, location, name)?;
  if state.intersects(SelectorParsingState::AFTER_WEBKIT_SCROLLBAR) {
    if !pseudo_class.is_valid_after_webkit_scrollbar() {
      return Err(location.new_custom_error(SelectorParseErrorKind::InvalidPseudoClassAfterWebKitScrollbar));
    }
  } else if state.intersects(SelectorParsingState::AFTER_PSEUDO_ELEMENT) {
    if !pseudo_class.is_user_action_state() {
      return Err(location.new_custom_error(SelectorParseErrorKind::InvalidPseudoClassAfterPseudoElement));
    }
  } else if !pseudo_class.is_valid_before_webkit_scrollbar() {
    return Err(location.new_custom_error(SelectorParseErrorKind::InvalidPseudoClassBeforeWebKitScrollbar));
  }
  Ok(Component::NonTSPseudoClass(pseudo_class))
}

// NB: pub module in order to access the DummyParser
#[cfg(test)]
pub mod tests {
  use super::*;
  use crate::builder::SelectorFlags;
  use crate::parser;
  use cssparser::{serialize_identifier, serialize_string, Parser as CssParser, ParserInput, ToCss};
  use std::collections::HashMap;
  use std::fmt;

  #[derive(Clone, Debug, Eq, PartialEq, Hash)]
  pub enum PseudoClass {
    Hover,
    Active,
    Lang(String),
  }

  #[derive(Clone, Debug, Eq, PartialEq, Hash)]
  pub enum PseudoElement {
    Before,
    After,
  }

  impl<'i> parser::PseudoElement<'i> for PseudoElement {
    type Impl = DummySelectorImpl;

    fn accepts_state_pseudo_classes(&self) -> bool {
      true
    }

    fn valid_after_slotted(&self) -> bool {
      true
    }
  }

  impl<'i> parser::NonTSPseudoClass<'i> for PseudoClass {
    type Impl = DummySelectorImpl;

    #[inline]
    fn is_active_or_hover(&self) -> bool {
      matches!(*self, PseudoClass::Active | PseudoClass::Hover)
    }

    #[inline]
    fn is_user_action_state(&self) -> bool {
      self.is_active_or_hover()
    }
  }

  impl ToCss for PseudoClass {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
      W: fmt::Write,
    {
      match *self {
        PseudoClass::Hover => dest.write_str(":hover"),
        PseudoClass::Active => dest.write_str(":active"),
        PseudoClass::Lang(ref lang) => {
          dest.write_str(":lang(")?;
          serialize_identifier(lang, dest)?;
          dest.write_char(')')
        }
      }
    }
  }

  impl ToCss for PseudoElement {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
      W: fmt::Write,
    {
      match *self {
        PseudoElement::Before => dest.write_str("::before"),
        PseudoElement::After => dest.write_str("::after"),
      }
    }
  }

  #[derive(Clone, Debug, PartialEq)]
  pub struct DummySelectorImpl;

  #[derive(Default)]
  pub struct DummyParser {
    default_ns: Option<DummyAtom>,
    ns_prefixes: HashMap<DummyAtom, DummyAtom>,
  }

  impl DummyParser {
    fn default_with_namespace(default_ns: DummyAtom) -> DummyParser {
      DummyParser {
        default_ns: Some(default_ns),
        ns_prefixes: Default::default(),
      }
    }
  }

  impl<'i> SelectorImpl<'i> for DummySelectorImpl {
    type ExtraMatchingData = ();
    type AttrValue = DummyAttrValue;
    type Identifier = DummyAtom;
    type LocalName = DummyAtom;
    type NamespaceUrl = DummyAtom;
    type NamespacePrefix = DummyAtom;
    type BorrowedLocalName = DummyAtom;
    type BorrowedNamespaceUrl = DummyAtom;
    type NonTSPseudoClass = PseudoClass;
    type PseudoElement = PseudoElement;
    type VendorPrefix = u8;
  }

  #[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
  pub struct DummyAttrValue(String);

  impl ToCss for DummyAttrValue {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
      W: fmt::Write,
    {
      serialize_string(&self.0, dest)
    }
  }

  impl AsRef<str> for DummyAttrValue {
    fn as_ref(&self) -> &str {
      self.0.as_ref()
    }
  }

  impl<'a> From<&'a str> for DummyAttrValue {
    fn from(string: &'a str) -> Self {
      Self(string.into())
    }
  }

  impl<'a> From<std::borrow::Cow<'a, str>> for DummyAttrValue {
    fn from(string: std::borrow::Cow<'a, str>) -> Self {
      Self(string.to_string())
    }
  }

  impl<'a> From<CowRcStr<'a>> for DummyAttrValue {
    fn from(string: CowRcStr<'a>) -> Self {
      Self(string.to_string())
    }
  }

  #[derive(Clone, Debug, Default, Eq, Hash, PartialEq)]
  pub struct DummyAtom(String);

  impl ToCss for DummyAtom {
    fn to_css<W>(&self, dest: &mut W) -> fmt::Result
    where
      W: fmt::Write,
    {
      serialize_identifier(&self.0, dest)
    }
  }

  impl From<String> for DummyAtom {
    fn from(string: String) -> Self {
      DummyAtom(string)
    }
  }

  impl<'a> From<&'a str> for DummyAtom {
    fn from(string: &'a str) -> Self {
      DummyAtom(string.into())
    }
  }

  impl<'a> From<CowRcStr<'a>> for DummyAtom {
    fn from(string: CowRcStr<'a>) -> Self {
      DummyAtom(string.to_string())
    }
  }

  impl AsRef<str> for DummyAtom {
    fn as_ref(&self) -> &str {
      self.0.as_ref()
    }
  }

  impl<'a> From<std::borrow::Cow<'a, str>> for DummyAtom {
    fn from(string: std::borrow::Cow<'a, str>) -> Self {
      Self(string.to_string())
    }
  }

  impl<'i> Parser<'i> for DummyParser {
    type Impl = DummySelectorImpl;
    type Error = SelectorParseErrorKind<'i>;

    fn parse_slotted(&self) -> bool {
      true
    }

    fn parse_is_and_where(&self) -> bool {
      true
    }

    fn is_and_where_error_recovery(&self) -> ParseErrorRecovery {
      ParseErrorRecovery::DiscardList
    }

    fn parse_part(&self) -> bool {
      true
    }

    fn parse_non_ts_pseudo_class(
      &self,
      location: SourceLocation,
      name: CowRcStr<'i>,
    ) -> Result<PseudoClass, SelectorParseError<'i>> {
      match_ignore_ascii_case! { &name,
          "hover" => return Ok(PseudoClass::Hover),
          "active" => return Ok(PseudoClass::Active),
          _ => {}
      }
      Err(location.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoClass(name)))
    }

    fn parse_non_ts_functional_pseudo_class<'t>(
      &self,
      name: CowRcStr<'i>,
      parser: &mut CssParser<'i, 't>,
    ) -> Result<PseudoClass, SelectorParseError<'i>> {
      match_ignore_ascii_case! { &name,
          "lang" => {
              let lang = parser.expect_ident_or_string()?.as_ref().to_owned();
              return Ok(PseudoClass::Lang(lang));
          },
          _ => {}
      }
      Err(parser.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoClass(name)))
    }

    fn parse_pseudo_element(
      &self,
      location: SourceLocation,
      name: CowRcStr<'i>,
    ) -> Result<PseudoElement, SelectorParseError<'i>> {
      match_ignore_ascii_case! { &name,
          "before" => return Ok(PseudoElement::Before),
          "after" => return Ok(PseudoElement::After),
          _ => {}
      }
      Err(location.new_custom_error(SelectorParseErrorKind::UnsupportedPseudoElement(name)))
    }

    fn default_namespace(&self) -> Option<DummyAtom> {
      self.default_ns.clone()
    }

    fn namespace_for_prefix(&self, prefix: &DummyAtom) -> Option<DummyAtom> {
      self.ns_prefixes.get(prefix).cloned()
    }
  }

  fn parse<'i>(input: &'i str) -> Result<SelectorList<DummySelectorImpl>, SelectorParseError<'i>> {
    parse_ns(input, &DummyParser::default())
  }

  // fn parse_expected<'i, 'a>(
  //   input: &'i str,
  //   expected: Option<&'a str>,
  // ) -> Result<SelectorList<'i, DummySelectorImpl>, SelectorParseError<'i>> {
  //   parse_ns_expected(input, &DummyParser::default(), expected)
  // }

  fn parse_ns<'i>(
    input: &'i str,
    parser: &DummyParser,
  ) -> Result<SelectorList<'i, DummySelectorImpl>, SelectorParseError<'i>> {
    parse_ns_expected(input, parser, None)
  }

  fn parse_ns_expected<'i, 'a>(
    input: &'i str,
    parser: &DummyParser,
    expected: Option<&'a str>,
  ) -> Result<SelectorList<'i, DummySelectorImpl>, SelectorParseError<'i>> {
    let mut parser_input = ParserInput::new(input);
    let result = SelectorList::parse(
      parser,
      &mut CssParser::new(&mut parser_input),
      ParseErrorRecovery::DiscardList,
      NestingRequirement::None,
    );
    if let Ok(ref selectors) = result {
      assert_eq!(selectors.0.len(), 1);
      // We can't assume that the serialized parsed selector will equal
      // the input; for example, if there is no default namespace, '*|foo'
      // should serialize to 'foo'.
      assert_eq!(
        selectors.0[0].to_css_string(),
        match expected {
          Some(x) => x,
          None => input,
        }
      );
    }
    result
  }

  fn specificity(a: u32, b: u32, c: u32) -> u32 {
    a << 20 | b << 10 | c
  }

  #[test]
  fn test_empty() {
    let mut input = ParserInput::new(":empty");
    let list = SelectorList::parse(
      &DummyParser::default(),
      &mut CssParser::new(&mut input),
      ParseErrorRecovery::DiscardList,
      NestingRequirement::None,
    );
    assert!(list.is_ok());
  }

  const MATHML: &str = "http://www.w3.org/1998/Math/MathML";
  const SVG: &str = "http://www.w3.org/2000/svg";

  #[test]
  fn test_parsing() {
    assert!(parse("").is_err());
    assert!(parse(":lang(4)").is_err());
    assert!(parse(":lang(en US)").is_err());
    assert_eq!(
      parse("EeÉ"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![Component::LocalName(LocalName {
          name: DummyAtom::from("EeÉ"),
          lower_name: DummyAtom::from("eeÉ"),
        })],
        specificity(0, 0, 1),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse("|e"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::ExplicitNoNamespace,
          Component::LocalName(LocalName {
            name: DummyAtom::from("e"),
            lower_name: DummyAtom::from("e"),
          }),
        ],
        specificity(0, 0, 1),
        Default::default(),
      )]))
    );
    // When the default namespace is not set, *| should be elided.
    // https://github.com/servo/servo/pull/17537
    // assert_eq!(
    //   parse_expected("*|e", Some("e")),
    //   Ok(SelectorList::from_vec(vec![Selector::from_vec(
    //     vec![Component::LocalName(LocalName {
    //       name: DummyAtom::from("e"),
    //       lower_name: DummyAtom::from("e"),
    //     })],
    //     specificity(0, 0, 1),
    //     Default::default(),
    //   )]))
    // );
    // When the default namespace is set, *| should _not_ be elided (as foo
    // is no longer equivalent to *|foo--the former is only for foo in the
    // default namespace).
    // https://github.com/servo/servo/issues/16020
    // assert_eq!(
    //   parse_ns(
    //     "*|e",
    //     &DummyParser::default_with_namespace(DummyAtom::from("https://mozilla.org"))
    //   ),
    //   Ok(SelectorList::from_vec(vec![Selector::from_vec(
    //     vec![
    //       Component::ExplicitAnyNamespace,
    //       Component::LocalName(LocalName {
    //         name: DummyAtom::from("e"),
    //         lower_name: DummyAtom::from("e"),
    //       }),
    //     ],
    //     specificity(0, 0, 1),
    //     Default::default(),
    //   )]))
    // );
    assert_eq!(
      parse("*"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![Component::ExplicitUniversalType],
        specificity(0, 0, 0),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse("|*"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![Component::ExplicitNoNamespace, Component::ExplicitUniversalType,],
        specificity(0, 0, 0),
        Default::default(),
      )]))
    );
    // assert_eq!(
    //   parse_expected("*|*", Some("*")),
    //   Ok(SelectorList::from_vec(vec![Selector::from_vec(
    //     vec![Component::ExplicitUniversalType],
    //     specificity(0, 0, 0),
    //     Default::default(),
    //   )]))
    // );
    assert_eq!(
      parse_ns(
        "*|*",
        &DummyParser::default_with_namespace(DummyAtom::from("https://mozilla.org"))
      ),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![Component::ExplicitAnyNamespace, Component::ExplicitUniversalType,],
        specificity(0, 0, 0),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse(".foo:lang(en-US)"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::Class(DummyAtom::from("foo")),
          Component::NonTSPseudoClass(PseudoClass::Lang("en-US".to_owned())),
        ],
        specificity(0, 2, 0),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse("#bar"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![Component::ID(DummyAtom::from("bar"))],
        specificity(1, 0, 0),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse("e.foo#bar"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::LocalName(LocalName {
            name: DummyAtom::from("e"),
            lower_name: DummyAtom::from("e"),
          }),
          Component::Class(DummyAtom::from("foo")),
          Component::ID(DummyAtom::from("bar")),
        ],
        specificity(1, 1, 1),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse("e.foo #bar"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::LocalName(LocalName {
            name: DummyAtom::from("e"),
            lower_name: DummyAtom::from("e"),
          }),
          Component::Class(DummyAtom::from("foo")),
          Component::Combinator(Combinator::Descendant),
          Component::ID(DummyAtom::from("bar")),
        ],
        specificity(1, 1, 1),
        Default::default(),
      )]))
    );
    // Default namespace does not apply to attribute selectors
    // https://github.com/mozilla/servo/pull/1652
    let mut parser = DummyParser::default();
    assert_eq!(
      parse_ns("[Foo]", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![Component::AttributeInNoNamespaceExists {
          local_name: DummyAtom::from("Foo"),
          local_name_lower: DummyAtom::from("foo"),
        }],
        specificity(0, 1, 0),
        Default::default(),
      )]))
    );
    assert!(parse_ns("svg|circle", &parser).is_err());
    parser.ns_prefixes.insert(DummyAtom("svg".into()), DummyAtom(SVG.into()));
    assert_eq!(
      parse_ns("svg|circle", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::Namespace(DummyAtom("svg".into()), SVG.into()),
          Component::LocalName(LocalName {
            name: DummyAtom::from("circle"),
            lower_name: DummyAtom::from("circle"),
          }),
        ],
        specificity(0, 0, 1),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse_ns("svg|*", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::Namespace(DummyAtom("svg".into()), SVG.into()),
          Component::ExplicitUniversalType,
        ],
        specificity(0, 0, 0),
        Default::default(),
      )]))
    );
    // Default namespace does not apply to attribute selectors
    // https://github.com/mozilla/servo/pull/1652
    // but it does apply to implicit type selectors
    // https://github.com/servo/rust-selectors/pull/82
    parser.default_ns = Some(MATHML.into());
    assert_eq!(
      parse_ns("[Foo]", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::DefaultNamespace(MATHML.into()),
          Component::AttributeInNoNamespaceExists {
            local_name: DummyAtom::from("Foo"),
            local_name_lower: DummyAtom::from("foo"),
          },
        ],
        specificity(0, 1, 0),
        Default::default(),
      )]))
    );
    // Default namespace does apply to type selectors
    assert_eq!(
      parse_ns("e", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::DefaultNamespace(MATHML.into()),
          Component::LocalName(LocalName {
            name: DummyAtom::from("e"),
            lower_name: DummyAtom::from("e"),
          }),
        ],
        specificity(0, 0, 1),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse_ns("*", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::DefaultNamespace(MATHML.into()),
          Component::ExplicitUniversalType,
        ],
        specificity(0, 0, 0),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse_ns("*|*", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![Component::ExplicitAnyNamespace, Component::ExplicitUniversalType,],
        specificity(0, 0, 0),
        Default::default(),
      )]))
    );
    // Default namespace applies to universal and type selectors inside :not and :matches,
    // but not otherwise.
    assert_eq!(
      parse_ns(":not(.cl)", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::DefaultNamespace(MATHML.into()),
          Component::Negation(
            vec![Selector::from_vec(
              vec![Component::Class(DummyAtom::from("cl"))],
              specificity(0, 1, 0),
              Default::default(),
            )]
            .into_boxed_slice()
          ),
        ],
        specificity(0, 1, 0),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse_ns(":not(*)", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::DefaultNamespace(MATHML.into()),
          Component::Negation(
            vec![Selector::from_vec(
              vec![
                Component::DefaultNamespace(MATHML.into()),
                Component::ExplicitUniversalType,
              ],
              specificity(0, 0, 0),
              Default::default(),
            )]
            .into_boxed_slice(),
          ),
        ],
        specificity(0, 0, 0),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse_ns(":not(e)", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::DefaultNamespace(MATHML.into()),
          Component::Negation(
            vec![Selector::from_vec(
              vec![
                Component::DefaultNamespace(MATHML.into()),
                Component::LocalName(LocalName {
                  name: DummyAtom::from("e"),
                  lower_name: DummyAtom::from("e"),
                }),
              ],
              specificity(0, 0, 1),
              Default::default(),
            ),]
            .into_boxed_slice()
          ),
        ],
        specificity(0, 0, 1),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse("[attr|=\"foo\"]"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![Component::AttributeInNoNamespace {
          local_name: DummyAtom::from("attr"),
          operator: AttrSelectorOperator::DashMatch,
          value: DummyAttrValue::from("foo"),
          never_matches: false,
          case_sensitivity: ParsedCaseSensitivity::CaseSensitive,
        }],
        specificity(0, 1, 0),
        Default::default(),
      )]))
    );
    // https://github.com/mozilla/servo/issues/1723
    assert_eq!(
      parse("::before"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::Combinator(Combinator::PseudoElement),
          Component::PseudoElement(PseudoElement::Before),
        ],
        specificity(0, 0, 1),
        SelectorFlags::HAS_PSEUDO,
      )]))
    );
    assert_eq!(
      parse("::before:hover"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::Combinator(Combinator::PseudoElement),
          Component::PseudoElement(PseudoElement::Before),
          Component::NonTSPseudoClass(PseudoClass::Hover),
        ],
        specificity(0, 1, 1),
        SelectorFlags::HAS_PSEUDO,
      )]))
    );
    assert_eq!(
      parse("::before:hover:hover"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::Combinator(Combinator::PseudoElement),
          Component::PseudoElement(PseudoElement::Before),
          Component::NonTSPseudoClass(PseudoClass::Hover),
          Component::NonTSPseudoClass(PseudoClass::Hover),
        ],
        specificity(0, 2, 1),
        SelectorFlags::HAS_PSEUDO,
      )]))
    );
    assert!(parse("::before:hover:lang(foo)").is_err());
    assert!(parse("::before:hover .foo").is_err());
    assert!(parse("::before .foo").is_err());
    assert!(parse("::before ~ bar").is_err());
    assert!(parse("::before:active").is_ok());

    // https://github.com/servo/servo/issues/15335
    assert!(parse(":: before").is_err());
    assert_eq!(
      parse("div ::after"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::LocalName(LocalName {
            name: DummyAtom::from("div"),
            lower_name: DummyAtom::from("div"),
          }),
          Component::Combinator(Combinator::Descendant),
          Component::Combinator(Combinator::PseudoElement),
          Component::PseudoElement(PseudoElement::After),
        ],
        specificity(0, 0, 2),
        SelectorFlags::HAS_PSEUDO,
      )]))
    );
    assert_eq!(
      parse("#d1 > .ok"),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![
          Component::ID(DummyAtom::from("d1")),
          Component::Combinator(Combinator::Child),
          Component::Class(DummyAtom::from("ok")),
        ],
        (1 << 20) + (1 << 10) + (0 << 0),
        Default::default(),
      )]))
    );
    parser.default_ns = None;
    assert!(parse(":not(#provel.old)").is_ok());
    assert!(parse(":not(#provel > old)").is_ok());
    assert!(parse("table[rules]:not([rules=\"none\"]):not([rules=\"\"])").is_ok());
    // https://github.com/servo/servo/issues/16017
    assert_eq!(
      parse_ns(":not(*)", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![Component::Negation(
          vec![Selector::from_vec(
            vec![Component::ExplicitUniversalType],
            specificity(0, 0, 0),
            Default::default(),
          )]
          .into_boxed_slice()
        )],
        specificity(0, 0, 0),
        Default::default(),
      )]))
    );
    assert_eq!(
      parse_ns(":not(|*)", &parser),
      Ok(SelectorList::from_vec(vec![Selector::from_vec(
        vec![Component::Negation(
          vec![Selector::from_vec(
            vec![Component::ExplicitNoNamespace, Component::ExplicitUniversalType,],
            specificity(0, 0, 0),
            Default::default(),
          )]
          .into_boxed_slice(),
        )],
        specificity(0, 0, 0),
        Default::default(),
      )]))
    );
    // *| should be elided if there is no default namespace.
    // https://github.com/servo/servo/pull/17537
    // assert_eq!(
    //   parse_ns_expected(":not(*|*)", &parser, Some(":not(*)")),
    //   Ok(SelectorList::from_vec(vec![Selector::from_vec(
    //     vec![Component::Negation(
    //       vec![Selector::from_vec(
    //         vec![Component::ExplicitUniversalType],
    //         specificity(0, 0, 0),
    //         Default::default()
    //       )]
    //       .into_boxed_slice()
    //     )],
    //     specificity(0, 0, 0),
    //     Default::default(),
    //   )]))
    // );

    assert!(parse("::slotted()").is_err());
    assert!(parse("::slotted(div)").is_ok());
    assert!(parse("::slotted(div).foo").is_err());
    assert!(parse("::slotted(div + bar)").is_err());
    assert!(parse("::slotted(div) + foo").is_err());

    assert!(parse("::part()").is_err());
    assert!(parse("::part(42)").is_err());
    assert!(parse("::part(foo bar)").is_ok());
    assert!(parse("::part(foo):hover").is_ok());
    assert!(parse("::part(foo) + bar").is_err());

    assert!(parse("div ::slotted(div)").is_ok());
    assert!(parse("div + slot::slotted(div)").is_ok());
    assert!(parse("div + slot::slotted(div.foo)").is_ok());
    assert!(parse("slot::slotted(div,foo)::first-line").is_err());
    assert!(parse("::slotted(div)::before").is_ok());
    assert!(parse("slot::slotted(div,foo)").is_err());

    assert!(parse("foo:where()").is_err());
    assert!(parse("foo:where(div, foo, .bar baz)").is_ok());
    assert!(parse("foo:where(::before)").is_err());
  }

  #[test]
  fn test_pseudo_iter() {
    let selector = &parse("q::before").unwrap().0[0];
    assert!(!selector.is_universal());
    let mut iter = selector.iter();
    assert_eq!(iter.next(), Some(&Component::PseudoElement(PseudoElement::Before)));
    assert_eq!(iter.next(), None);
    let combinator = iter.next_sequence();
    assert_eq!(combinator, Some(Combinator::PseudoElement));
    assert!(matches!(iter.next(), Some(&Component::LocalName(..))));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_sequence(), None);
  }

  #[test]
  fn test_universal() {
    let selector = &parse_ns(
      "*|*::before",
      &DummyParser::default_with_namespace(DummyAtom::from("https://mozilla.org")),
    )
    .unwrap()
    .0[0];
    assert!(selector.is_universal());
  }

  #[test]
  fn test_empty_pseudo_iter() {
    let selector = &parse("::before").unwrap().0[0];
    assert!(selector.is_universal());
    let mut iter = selector.iter();
    assert_eq!(iter.next(), Some(&Component::PseudoElement(PseudoElement::Before)));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_sequence(), Some(Combinator::PseudoElement));
    assert_eq!(iter.next(), None);
    assert_eq!(iter.next_sequence(), None);
  }

  struct TestVisitor {
    seen: Vec<String>,
  }

  impl<'i> SelectorVisitor<'i> for TestVisitor {
    type Impl = DummySelectorImpl;

    fn visit_simple_selector(&mut self, s: &Component<DummySelectorImpl>) -> bool {
      let mut dest = String::new();
      s.to_css(&mut dest).unwrap();
      self.seen.push(dest);
      true
    }
  }

  #[test]
  fn visitor() {
    let mut test_visitor = TestVisitor { seen: vec![] };
    parse(":not(:hover) ~ label").unwrap().0[0].visit(&mut test_visitor);
    assert!(test_visitor.seen.contains(&":hover".into()));

    let mut test_visitor = TestVisitor { seen: vec![] };
    parse("::before:hover").unwrap().0[0].visit(&mut test_visitor);
    assert!(test_visitor.seen.contains(&":hover".into()));
  }
}
