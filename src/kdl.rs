use crate::val::init;
use polonius_the_crab::*;

pub use ::kdl::*;
pub use kdl_schema::Schema;
pub use kdl_schema_check::{CheckExt, CheckFailure};

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
  V: Into<KdlValue> + for<'a> PartialEq<&'a KdlValue>,
{
  pub fn new<R>(name: N, entry: (K, R)) -> Self
  where
    R: ValueRepr<Eq = V>,
  {
    Self { name, entry: (entry.0, entry.1.into()) }
  }

  /// If `children` contains a node that matches `self`, returns a reference to it.
  /// Otherwise, converts `self` into a node, applies `builder` to it,
  /// pushes it into `children` and returns a reference to it.
  pub fn get_or_insert<F>(self, mut children: &mut Vec<KdlNode>, builder: F) -> &mut KdlNode
  where
    F: FnOnce(&mut KdlNode) -> (),
  {
    polonius!(|children| -> &'polonius mut KdlNode {
      if let Some(node) = children.iter_mut().find(|child| self == **child) {
        polonius_return!(node);
      }
    });
    children.push(init(self.into(), builder));
    children.last_mut().unwrap()
  }
}

impl<N, K, V> PartialEq<KdlNode> for NodeId<N, K, V>
where
  N: Into<KdlIdentifier> + for<'a> PartialEq<&'a str>,
  K: Into<NodeKey> + Copy,
  V: Into<KdlValue> + for<'a> PartialEq<&'a KdlValue>,
{
  fn eq(&self, node: &KdlNode) -> bool {
    self.name == node.name().value()
      && node
        .get(self.entry.0)
        .is_some_and(|entry| self.entry.1 == entry.value())
  }
}

impl<N, K, V> From<NodeId<N, K, V>> for KdlNode
where
  N: Into<KdlIdentifier> + for<'a> PartialEq<&'a str>,
  K: Into<NodeKey>,
  V: Into<KdlValue>,
{
  fn from(desc: NodeId<N, K, V>) -> Self {
    init(KdlNode::new(desc.name), |node| {
      node.insert(desc.entry.0, desc.entry.1);
    })
  }
}

/// Types that can be converted into a `KdlValue` and have a newtype that
/// implements `PartialEq<&KdlValue>`.
pub trait ValueRepr: Sized + Into<Self::Eq> {
  type Eq: Into<KdlValue> + for<'a> PartialEq<&'a KdlValue>;
}

impl<'a> ValueRepr for &'a str {
  type Eq = &'a Str;
}

/// Newtype for `str` that supports `PartialEq<&KdlValue>`
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct Str(str);

impl Str {
  pub fn new(str: &str) -> &Self {
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

impl PartialEq<&KdlValue> for &Str {
  fn eq(&self, other: &&KdlValue) -> bool {
    Some(&self.0) == other.as_string()
  }
}
