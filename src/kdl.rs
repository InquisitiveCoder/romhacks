use crate::{crc, mem};
pub use kdl::*;
pub use kdl_schema::Schema;
pub use kdl_schema_check::{CheckExt, CheckFailure};
use polonius_the_crab::*;

pub mod prelude {
  pub use kdl_schema_check::CheckExt;
}

pub fn unwrap_children(node: &KdlNode) -> &[KdlNode] {
  node.children().unwrap().nodes()
}

/// A node name and identifying entry.
#[derive(Debug, Clone, Copy)]
pub struct NodeId<N, K, V> {
  name: N,
  entry: (K, V),
}

impl<N, K, V> NodeId<N, K, V>
where
  N: Into<KdlIdentifier> + for<'a> PartialEq<&'a str>,
  K: Into<NodeKey> + Copy,
  V: Into<KdlValue> + PartialEq<KdlValue>,
{
  pub fn new<R>(name: N, entry: (K, R)) -> Self
  where
    R: ValueRepr<Repr = V>,
  {
    Self { name, entry: (entry.0, entry.1.into()) }
  }

  /// If `children` contains a node that matches `self`, returns a reference to it.
  /// Otherwise, converts `self` into a node, applies `builder` to it,
  /// pushes it into `children` and returns a reference to it.
  pub fn get_or_insert<F>(self, mut children: &mut Vec<KdlNode>, builder: F) -> &mut KdlNode
  where
    F: FnOnce(&mut KdlNode),
  {
    polonius!(|children| -> &'polonius mut KdlNode {
      if let Some(node) = children.iter_mut().find(|child| self == **child) {
        polonius_return!(node);
      }
    });
    children.push(mem::init(self.into(), builder));
    children.last_mut().unwrap()
  }
}

impl<N, K, V> PartialEq<KdlNode> for NodeId<N, K, V>
where
  N: Into<KdlIdentifier> + for<'a> PartialEq<&'a str>,
  K: Into<NodeKey> + Copy,
  V: Into<KdlValue> + PartialEq<KdlValue>,
{
  fn eq(&self, node: &KdlNode) -> bool {
    self.name == node.name().value()
      && node
        .get(self.entry.0)
        .is_some_and(|entry| self.entry.1 == *entry.value())
  }
}

impl<N, K, V> From<NodeId<N, K, V>> for KdlNode
where
  N: Into<KdlIdentifier> + for<'a> PartialEq<&'a str>,
  K: Into<NodeKey>,
  V: Into<KdlValue>,
{
  fn from(desc: NodeId<N, K, V>) -> Self {
    mem::init(KdlNode::new(desc.name), |node| {
      node.insert(desc.entry.0, desc.entry.1);
    })
  }
}

/// Types that can be converted into a `KdlValue` and have a newtype that
/// implements `PartialEq<KdlValue>`.
pub trait ValueRepr: Sized + Into<Self::Repr> {
  type Repr: Into<KdlValue> + PartialEq<KdlValue>;
}

impl<'a> ValueRepr for &'a str {
  type Repr = &'a Str;
}

impl ValueRepr for crc::Crc32 {
  type Repr = Self;
}

impl From<crc::Crc32> for KdlValue {
  fn from(crc32: crc::Crc32) -> Self {
    KdlValue::Base16(crc32.value().into())
  }
}

impl PartialEq<KdlValue> for crc::Crc32 {
  fn eq(&self, other: &KdlValue) -> bool {
    Some(self.value() as i64) == other.as_i64()
  }
}

/// Newtype for `str` that supports `PartialEq<&KdlValue>`
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Str(str);

impl Str {
  pub fn new(str: &str) -> &Self {
    // Str is just a newtype for str, so this is always safe.
    unsafe { &*(str as *const str as *const Self) }
  }
}

impl<'a> From<&'a str> for &'a Str {
  fn from(str: &'a str) -> Self {
    Str::new(str)
  }
}

impl From<&Str> for KdlValue {
  fn from(value: &Str) -> Self {
    KdlValue::from(&value.0)
  }
}

impl std::ops::Deref for Str {
  type Target = str;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl AsRef<str> for &Str {
  fn as_ref(&self) -> &str {
    &self.0
  }
}

impl PartialEq<KdlValue> for &Str {
  fn eq(&self, other: &KdlValue) -> bool {
    Some(&self.0) == other.as_string()
  }
}
