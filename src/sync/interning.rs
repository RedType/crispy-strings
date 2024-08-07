use crate::sync::{
  error::InternError,
  trie::{Span, Trie},
};
use parking_lot::{Condvar, Mutex};
use std::{
  collections::HashMap,
  fmt,
  ops::Deref,
  panic::Location,
  sync::Arc,
  thread::{self, ThreadId},
};

/// The interner, which is where the underlying data store and index lives.
/// [Intern]s keep a reference to this struct, so it can be tossed aside when
/// no longer needed.
pub struct Interner {
  cond: Condvar,
  internal: Mutex<InternerInternal>,
}

pub struct InternerInternal {
  index: Mutex<Trie<char>>,
  store: Mutex<String>,
  refs: usize,
  local_refs: HashMap<ThreadId, usize>,
  last_ref: Option<&'static Location<'static>>,
}

impl InternerInternal {
  fn intern_uncontested<S: AsRef<str>>(&mut self, s: S) -> Span {
    let index = unsafe { &mut *self.index.data_ptr() };
    let store = unsafe { &mut *self.store.data_ptr() };
    let span = index.get(s.as_ref().chars()).unwrap_or_else(|| {
      let start = store.len();
      store.push_str(s.as_ref());
      index.insert(s.as_ref().chars(), start)
    });

    span
  }
}

impl Interner {
  /// Constructs a new Interner
  pub fn new() -> Arc<Self> {
    Arc::new(Self {
      cond: Condvar::new(),
      internal: Mutex::new(InternerInternal {
        index: Mutex::new(Trie::new()),
        store: Mutex::new(String::new()),
        refs: 0,
        local_refs: HashMap::new(),
        last_ref: None,
      }),
    })
  }

  /// Creates an interned string
  ///
  /// # Safety
  ///
  /// If there are any existing InternRefs in other threads, this method will
  /// wait for them to be dropped. If this thread has any InternRefs, then
  /// this method will panic and try to tell you where the last one was
  /// created.
  ///
  /// # Examples
  ///
  /// ```
  /// use str_interning::sync::Interner;
  ///
  /// let interner = Interner::new();
  ///
  /// let hello = interner.intern("hello");
  /// let hello2 = interner.intern("hello");
  ///
  /// assert_eq!(hello, hello2);
  /// ```
  pub fn intern<S: AsRef<str>>(self: &Arc<Self>, s: S) -> Intern {
    let mut lock = self.internal.lock();

    // check local refs
    let key = thread::current().id();
    if let Some(local_refs) = lock.local_refs.get(&key) {
      if *local_refs > 0 {
        let lr = lock.last_ref.unwrap();
        panic!(
          concat!(
            "Cannot intern while holding an InternRef\n",
            " --> last InternRef created at {}:{}:{}",
          ),
          lr.file(),
          lr.line(),
          lr.column(),
        );
      }
    } else {
      lock.local_refs.insert(key, 0);
    }

    // check nonlocal refs
    if lock.refs > 0 {
      self.cond.wait(&mut lock);
    }

    Intern {
      span: lock.intern_uncontested(s),
      interner: Arc::clone(self),
    }
  }

  /// Creates an interned string
  ///
  /// # Safety
  ///
  /// If there are any existing InternRefs in other threads, this method will
  /// wait for them to be dropped. If this thread has any InternRefs, then
  /// this method will return an appropriate error.
  ///
  /// # Examples
  ///
  /// ```
  /// use str_interning::sync::Interner;
  ///
  /// let interner = Interner::new();
  ///
  /// let hello = interner.try_intern("hello").unwrap();
  /// let hello2 = interner.try_intern("hello").unwrap();
  ///
  /// assert_eq!(hello, hello2);
  /// ```
  pub fn try_intern<S: AsRef<str>>(
    self: &Arc<Self>,
    s: S,
  ) -> Result<Intern, InternError> {
    let mut lock = self.internal.lock();

    // check local refs
    let key = thread::current().id();
    if let Some(local_refs) = lock.local_refs.get(&key) {
      if *local_refs > 0 {
        return Err(InternError::OutstandingLocalRef(lock.last_ref.unwrap()));
      }
    } else {
      lock.local_refs.insert(key, 0);
    }

    // check nonlocal refs
    if lock.refs > 0 {
      self.cond.wait(&mut lock);
    }

    Ok(Intern {
      span: lock.intern_uncontested(s),
      interner: Arc::clone(self),
    })
  }
}

/// Represents a single interned string. This struct may be passed around
/// and cloned cheaply, and without regard for lifetimes. Created using
/// [Interner::intern] or [Interner::try_intern].
///
/// The string data that this [Intern] represents is only accessible through
/// the short-lived [InternRef] type, which can be produced by
/// [Intern::get_ref].
///
/// # Examples
///
/// ```
/// use str_interning::sync::Interner;
///
/// let interner = Interner::new();
///
/// let hello = interner.intern("hello");
/// let hello_ref = hello.get_ref();
///
/// assert_eq!(&*hello_ref, "hello");
/// ```
///
/// The following example will panic because the intern method was called
/// while an [InternRef] still exists.
///
/// ```should_panic
/// use str_interning::sync::Interner;
///
/// let interner = Interner::new();
/// let hello = interner.intern("hello");
/// let _hello_ref = hello.get_ref();
///
/// interner.intern("goodbye"); // this will cause a panic
/// ```
#[derive(Clone)]
pub struct Intern {
  span: Span,
  interner: Arc<Interner>,
}

impl Intern {
  /// Produces an [InternRef]. This is the only way to access the underlying
  /// string data that this [Intern] represents.
  ///
  /// # Safety
  ///
  /// While any [InternRef]s exist, no new [Intern]s can be produced.
  #[track_caller]
  pub fn get_ref(&self) -> InternRef<'_> {
    let mut lock = self.interner.internal.lock();

    // register last caller location for helpful panics
    lock.last_ref = Some(Location::caller());

    // increment refcounts
    lock.refs += 1;
    let key = thread::current().id();
    if let Some(refs) = lock.local_refs.get_mut(&key) {
      *refs += 1;
    } else {
      lock.local_refs.insert(key, 1);
    }

    let store = unsafe { &*lock.store.data_ptr() };

    InternRef {
      text: &store[self.span.clone()],
      interner: Arc::clone(&self.interner),
    }
  }
}

impl fmt::Debug for Intern {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("Intern")
      .field("span", &self.span)
      .field("text", &&*self.get_ref())
      .finish()
  }
}

impl fmt::Display for Intern {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(&*self.get_ref())
  }
}

impl PartialEq for Intern {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.interner, &other.interner) && self.span == other.span
  }
}
impl Eq for Intern {}

/// Allows access to the string that an [Intern] represents. This type is
/// not meant to be long-lived; you should only create them when needed and
/// drop them as soon as possible. This type is so powerful that its very
/// existence blocks all threads from creating new [Intern]s.
///
/// # Examples
///
/// ```
/// use str_interning::sync::Interner;
///
/// let interner = Interner::new();
/// let hello = interner.intern("hello");
///
/// let hello_ref = hello.get_ref();
///
/// assert_eq!(&*hello_ref, "hello");
/// ```
pub struct InternRef<'a> {
  text: &'a str,
  interner: Arc<Interner>,
}

impl Drop for InternRef<'_> {
  fn drop(&mut self) {
    let mut lock = self.interner.internal.lock();

    let key = thread::current().id();
    if let Some(local_refs) = lock.local_refs.get_mut(&key) {
      if *local_refs == 0 {
        panic!("Interner nonlocal reference count overflow");
      } else {
        *local_refs -= 1;
      }
    } else {
      unreachable!();
    }

    if lock.refs == 0 {
      panic!("Interner nonlocal reference count overflow");
    } else {
      lock.refs -= 1;
    }

    if lock.refs == 0 {
      self.interner.cond.notify_all();
    }
  }
}

impl fmt::Debug for InternRef<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_tuple("InternRef").field(&self.text).finish()
  }
}

impl fmt::Display for InternRef<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.text)
  }
}

impl Deref for InternRef<'_> {
  type Target = str;

  fn deref(&self) -> &Self::Target {
    self.text
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::ptr;

  #[test]
  fn interner() {
    let interner = Interner::new();
    let hello1 = interner.intern("hello");
    let goodbye = interner.intern("goodbye");
    let hello2 = interner.intern("hello");

    assert_eq!(&*hello1.get_ref(), "hello");
    assert_eq!(&*hello2.get_ref(), "hello");
    assert_eq!(&*goodbye.get_ref(), "goodbye");

    assert_eq!(*hello1.get_ref(), *hello2.get_ref());
    assert_ne!(*hello1.get_ref(), *goodbye.get_ref());

    assert!(ptr::addr_eq(&*hello1.get_ref(), &*hello2.get_ref()));
    assert!(!ptr::addr_eq(&*hello1.get_ref(), &*goodbye.get_ref()));
  }

  #[test]
  #[should_panic]
  fn interner_errors_while_holding_refs() {
    let interner = Interner::new();
    let hello = interner.intern("hello");
    let _hello_ref = hello.get_ref();
    interner.intern("hello"); // this should panic
  }
}
