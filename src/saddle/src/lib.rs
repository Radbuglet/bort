use std::marker::PhantomData;

// === CFG === //

cfgenius::define! {
    pub IS_STATIC_VALIDATION_DISABLED = cfg(any(
        feature = "disable-static-validation",
        target_arch = "wasm32",
    ));
}

cfgenius::cond! {
    if macro(IS_STATIC_VALIDATION_DISABLED) {
        #[warn(deprecated)]
        #[allow(dead_code)]
        mod saddle {
            #[deprecated(note = "Saddle static validation is disabled in this build because the \
                                 `disable-static-validation` feature was enabled or because the \
                                 build target is a WebAssembly module.")]

            fn hack_to_issue_warning() {}

            fn whee() {
                hack_to_issue_warning();
            }
        }
    }
}

// === Markers === //

// Universe
mod universe {
    pub trait Private {
        const DEF_LOC: &'static str;
    }
}

pub trait Universe: Sized + 'static + universe::Private {}

#[doc(hidden)]
pub mod macro_internals_universe {
    pub use {
        super::{universe, universe::Private, Universe},
        std::{column, concat, file, line, primitive::str, stringify},
    };
}

#[macro_export]
macro_rules! universe {
    ($(
        $(#[$attr:meta])*
        $vis:vis $name:ident
    );* $(;)?) => {$(
        $(#[$attr])*
        #[non_exhaustive]
        $vis struct $name;

        $crate::macro_internals_universe::universe!(derive for $name);
    )*};
    ($(derive for $target:ty$(;)?)*) => {$(
        impl $crate::macro_internals_universe::Private for $target {
            const DEF_LOC: &'static $crate::macro_internals_universe::str = $crate::macro_internals_universe::concat!(
                "universe ",
                $crate::macro_internals_universe::stringify!($target),
                " defined at ",
                $crate::macro_internals_universe::file!(),
                ":",
                $crate::macro_internals_universe::line!(),
                ":",
                $crate::macro_internals_universe::column!(),
            );
        }

        impl $crate::macro_internals_universe::Universe for $target {}
    )*};
}

// ProcCollection
mod proc_collection {
    pub trait Private {
        const DEF_LOC: &'static str;
    }
}

pub trait ProcCollection: Sized + 'static + proc_collection::Private {}

#[doc(hidden)]
pub mod macro_internals_proc_collection {
    pub use {
        super::{proc_collection, proc_collection::Private, ProcCollection},
        std::{column, concat, file, line, primitive::str, stringify},
    };
}

#[macro_export]
macro_rules! proc_collection {
    ($(
        $(#[$attr:meta])*
        $vis:vis $name:ident
    );* $(;)?) => {$(
        $(#[$attr])*
        #[non_exhaustive]
        $vis struct $name;

        $crate::macro_internals_proc_collection::proc_collection!(derive for $name);
    )*};
    ($(derive for $target:ty$(;)?)*) => {$(
        impl $crate::macro_internals_proc_collection::Private for $target {
            const DEF_LOC: &'static $crate::macro_internals_proc_collection::str = $crate::macro_internals_proc_collection::concat!(
                "universe ",
                $crate::macro_internals_proc_collection::stringify!($target),
                " defined at ",
                $crate::macro_internals_proc_collection::file!(),
                ":",
                $crate::macro_internals_proc_collection::line!(),
                ":",
                $crate::macro_internals_proc_collection::column!(),
            );
        }

        impl $crate::macro_internals_proc_collection::ProcCollection for $target {}
    )*};
}

// === Global Token === //

#[derive(Debug)]
pub struct DangerousGlobalAccessToken {
    _private: (),
}

impl DangerousGlobalAccessToken {
    pub fn new() -> &'static mut Self {
        Box::leak(Box::new(DangerousGlobalAccessToken { _private: () }))
    }
}

impl<U, T> access::Private<access::Mutable, U, T> for DangerousGlobalAccessToken
where
    U: Universe,
    T: ?Sized + 'static,
{
}

impl<C: ProcCollection> can_call_collection::Private<C> for DangerousGlobalAccessToken {}

impl<C: ProcCollection> CanCallCollection<C> for DangerousGlobalAccessToken {
    fn as_dyn(&self) -> &dyn CanCallCollection<C> {
        self
    }

    fn as_dyn_mut(&mut self) -> &mut dyn CanCallCollection<C> {
        self
    }
}

// === Collection Token === //

mod can_call_collection {
    use super::*;

    pub trait Private<C: ProcCollection> {}
}

pub trait CanCallCollection<C: ProcCollection>: can_call_collection::Private<C> {
    fn as_dyn(&self) -> &dyn CanCallCollection<C>;

    fn as_dyn_mut(&mut self) -> &mut dyn CanCallCollection<C>;
}

#[doc(hidden)]
pub mod macro_internals_call_cx {
    pub use {super::CanCallCollection, std::marker::Sized};
}

#[macro_export]
macro_rules! call_cx {
    ($ty:ty $(,)?) => { dyn $crate::macro_internals_call_cx::CanCallCollection<$ty> };
    ($($ty:ty),*$(,)?) => { impl ?Sized $(+ $crate::macro_internals_call_cx::CanCallCollection<$ty>)* };
    (sized; $($ty:ty),*$(,)?) => {
        impl $crate::macro_internals_call_cx::Sized
            $(+ $crate::macro_internals_call_cx::CanCallCollection<$ty>)*
    };
}

// === Access Tokens === //

// Traits
mod access {
    use super::{
        validator::{ComponentType, ComponentTypeNames},
        *,
    };

    pub struct Immutable;

    pub struct Mutable;

    pub trait Private<M, U: Universe, T: ?Sized + 'static> {}

    impl<K, U, T> Private<Immutable, U, T> for K
    where
        K: Private<Mutable, U, T>,
        U: Universe,
        T: ?Sized + 'static,
    {
    }

    pub trait IterableAccessTrait {
        type AccessIter: Iterator<Item = (ComponentType, ComponentTypeNames, validator::Mutability)>;

        fn iter_access() -> Self::AccessIter;
    }
}

pub trait AccessRef<U: Universe, T: ?Sized + 'static>:
    access::Private<access::Immutable, U, T>
{
    fn as_dyn(&self) -> &dyn AccessRef<U, T>;

    fn as_dyn_mut(&mut self) -> &mut dyn AccessRef<U, T>;
}

pub trait AccessMut<U: Universe, T: ?Sized + 'static>:
    AccessRef<U, T> + access::Private<access::Mutable, U, T>
{
    fn as_dyn(&self) -> &dyn AccessMut<U, T>;

    fn as_dyn_mut(&mut self) -> &mut dyn AccessMut<U, T>;
}

impl<K, U, T> AccessRef<U, T> for K
where
    K: access::Private<access::Immutable, U, T>,
    U: Universe,
    T: ?Sized + 'static,
{
    fn as_dyn(&self) -> &dyn AccessRef<U, T> {
        DangerousGlobalAccessToken::new()
    }

    fn as_dyn_mut(&mut self) -> &mut dyn AccessRef<U, T> {
        DangerousGlobalAccessToken::new()
    }
}

impl<K, U, T> AccessMut<U, T> for K
where
    K: access::Private<access::Mutable, U, T>,
    U: Universe,
    T: ?Sized + 'static,
{
    fn as_dyn(&self) -> &dyn AccessMut<U, T> {
        DangerousGlobalAccessToken::new()
    }

    fn as_dyn_mut(&mut self) -> &mut dyn AccessMut<U, T> {
        DangerousGlobalAccessToken::new()
    }
}

// Builder
#[doc(hidden)]
pub mod macro_internals_access_cx {
    pub use {
        super::{
            access::{Immutable, IterableAccessTrait, Mutable, Private},
            access_cx,
            universe::Private as UniversePrivate,
            validator::{ComponentType, ComponentTypeNames, Mutability},
            DangerousGlobalAccessToken,
        },
        std::{
            any::{type_name, TypeId},
            compile_error, concat,
            iter::{IntoIterator, Iterator},
            stringify,
        },
    };

    pub trait Dummy {}

    impl<T: ?Sized> Dummy for T {}

    pub type TriChain<_Ignored, A, B> = <_Ignored as TriChainInner<A, B>>::Output;
    pub type ArrayIter<const N: usize> =
        std::array::IntoIter<(ComponentType, ComponentTypeNames, Mutability), N>;

    pub trait TriChainInner<A, B> {
        type Output;
    }

    impl<T: ?Sized, A, B> TriChainInner<A, B> for T {
        type Output = std::iter::Chain<A, B>;
    }

    pub const fn bind_and_return_one<T: ?Sized>() -> usize {
        1
    }
}

#[macro_export]
macro_rules! access_cx {
    ($(
        $(#[$attr:meta])*
        $vis:vis trait $name:ident$(: $($inherits:path),*$(,)?)? $(=
            $(
                $($kw:ident $component:ty),*$(,)?
                : $universe:ty
            ),* $(,)?
        )?
        ;
    )*) => {$(
        $(#[$attr])*
        $vis trait $name:
            $($($inherits + )*)?
            $($($(
                $crate::macro_internals_access_cx::Private<
                    $crate::macro_internals_access_cx::access_cx!(@__decode_kw $kw),
                    $universe,
                    $component,
                > +
            )*)*)?
            $crate::macro_internals_access_cx::Dummy
        {
            fn as_dyn(&self) -> &dyn $name;

            fn as_dyn_mut(&mut self) -> &mut dyn $name;
        }

        impl<K> $name for K
        where
            K: ?Sized $($(+ $inherits)*)?
            $($($(
                + $crate::macro_internals_access_cx::Private<
                    $crate::macro_internals_access_cx::access_cx!(@__decode_kw $kw),
                    $universe,
                    $component,
                >
            )*)*)?
        {
            fn as_dyn(&self) -> &dyn $name {
                $crate::macro_internals_access_cx::DangerousGlobalAccessToken::new()
            }

            fn as_dyn_mut(&mut self) -> &mut dyn $name {
                $crate::macro_internals_access_cx::DangerousGlobalAccessToken::new()
            }
        }

        impl $crate::macro_internals_access_cx::IterableAccessTrait for dyn $name {
            type AccessIter =
                // N.B. This construction is a bit weird. One may think that the first item in the
                // list is the inner-most iterator and the last item is the outermost, but this is
                // reversed because we use the third trailing parameter to define the type, not the
                // leading one!
                $($($crate::macro_internals_access_cx::TriChain<dyn $inherits, )*)?
                $crate::macro_internals_access_cx::ArrayIter<{
                    $($($($crate::macro_internals_access_cx::bind_and_return_one::<$component>() +)*)*)? 0
                }>
                $($(, <dyn $inherits as $crate::macro_internals_access_cx::IterableAccessTrait>::AccessIter> )*)?;

            fn iter_access() -> Self::AccessIter {
                let iter = $crate::macro_internals_access_cx::IntoIterator::into_iter([$($($(
                    (
                        $crate::macro_internals_access_cx::ComponentType {
                            universe: $crate::macro_internals_access_cx::TypeId::of::<$universe>(),
                            component: $crate::macro_internals_access_cx::TypeId::of::<$component>(),
                        },
                        $crate::macro_internals_access_cx::ComponentTypeNames {
                            universe: <$universe as $crate::macro_internals_access_cx::UniversePrivate>::DEF_LOC,
                            component: $crate::macro_internals_access_cx::type_name::<$component>(),
                        },
                        $crate::macro_internals_access_cx::access_cx!(@__decode_kw_as_enum $kw),
                    ),
                )*)*)?]);

                $($(
                    let iter = $crate::macro_internals_access_cx::Iterator::chain(
                        iter,
                        <dyn $inherits as $crate::macro_internals_access_cx::IterableAccessTrait>::iter_access(),
                    );
                )*)?

                iter
            }
        }
    )*};
    (
        $(
            $($kw:ident $component:ty),* $(,)?
            : $universe:ty
        ),* $(,)?

        $(; $($inherits:path),*$(,)?)?
    ) => {
        impl ?Sized
            $($(+ $inherits)*)?
            $($(+ $crate::macro_internals_access_cx::Private<
                    $crate::macro_internals_access_cx::access_cx!(@__decode_kw $kw),
                    $universe,
                    $component,
            >)*)*
    };
    (@__decode_kw ref) => { $crate::macro_internals_access_cx::Immutable };
    (@__decode_kw mut) => { $crate::macro_internals_access_cx::Mutable };
    (@__decode_kw_as_enum ref) => { $crate::macro_internals_access_cx::Mutability::Immutable };
    (@__decode_kw_as_enum mut) => { $crate::macro_internals_access_cx::Mutability::Mutable };
    (@__decode_kw $other:ident) => {
        $crate::macro_internals_access_cx::compile_error!($crate::macro_internals_access_cx::concat!(
            "Unknown mutability marker `",
            $crate::macro_internals_access_cx::stringify!($other),
            "`. Expected `mut` or `ref`.",
        ));
    }
}

// === Proc === //

mod proc {
    cfgenius::cond! {
        if not(macro(super::IS_STATIC_VALIDATION_DISABLED)) {
            use super::validator::Validator;
            use linkme::distributed_slice;

            #[distributed_slice]
            pub static PROCEDURE_REGISTRARS: [fn(&mut Validator)] = [..];
        }
    }
}

pub mod macro_internals_proc {
    use crate::CanCallCollection;

    pub use {
        super::{
            access::IterableAccessTrait, access_cx, call_cx,
            proc_collection::Private as ProcCollectionPrivate, validator::Validator,
            DangerousGlobalAccessToken, ProcCollection, IS_STATIC_VALIDATION_DISABLED,
        },
        cfgenius::cond,
        linkme::distributed_slice,
        partial_scope::partial_shadow,
        std::{any::TypeId, column, concat, file, line},
    };

    cfgenius::cond! {
        if not(macro(super::IS_STATIC_VALIDATION_DISABLED)) {
            pub use super::proc::PROCEDURE_REGISTRARS;
        }
    }

    pub trait CanCallCollectionExt<C: ProcCollection>: CanCallCollection<C> {
        fn __validate_collection_token(&mut self) -> CanCallCollectionTyProof<'_, C>;
    }

    pub struct CanCallCollectionTyProof<'a, C> {
        _private: ([&'a (); 0], [C; 0]),
    }

    impl<C: ProcCollection, K: ?Sized + CanCallCollection<C>> CanCallCollectionExt<C> for K {
        fn __validate_collection_token(&mut self) -> CanCallCollectionTyProof<'_, C> {
            CanCallCollectionTyProof { _private: ([], []) }
        }
    }

    pub fn validate_collection_token<C: ProcCollection>(
        cx: CanCallCollectionTyProof<'_, C>,
    ) -> CanCallCollectionTyProof<'_, C> {
        cx
    }
}

#[macro_export]
macro_rules! proc {
    (
        as $in_collection:ty[$in_collection_cx:expr] do
        $(
            (
                $access_cx_name:ident: [
                    $(
                        $($access_kw:ident $access_component:ty),* $(,)?
                        : $access_universe:ty
                    ),* $(,)?

                    $(; $($access_inherits:path),*$(,)?)?
                ],
                $collection_cx_name:ident: [
                    $($out_collection:ty),*$(,)?
                ]
                $(,)?
            ) {
                $($body:tt)*
            }
            $(,)?
        )*
    ) => {
        let mut __input = {
            use $crate::macro_internals_proc::CanCallCollectionExt as _;

            // Validate the collection token
            $crate::macro_internals_proc::validate_collection_token::<$in_collection>(
                $in_collection_cx.__validate_collection_token(),
            )
        };

        $($crate::macro_internals_proc::partial_shadow! {
            $access_cx_name, $collection_cx_name;

            let ($access_cx_name, $collection_cx_name) = {
                // Define a trait describing the set of components we're acquiring.
                $crate::macro_internals_proc::access_cx! {
                    trait ProcAccess$(: $($access_inherits),*)? = $(
                        $($access_kw $access_component),*
                        : $access_universe
                    ),*;
                };

                // Define a registration method
                $crate::macro_internals_proc::cond! {
                    if not(macro($crate::macro_internals_proc::IS_STATIC_VALIDATION_DISABLED)) {
                        #[$crate::macro_internals_proc::distributed_slice($crate::macro_internals_proc::PROCEDURE_REGISTRARS)]
                        static __THIS_PROCEDURE: fn(&mut $crate::macro_internals_proc::Validator) = |validator| {
                            validator.add_procedure(
                                /* collection: */ (
                                    $crate::macro_internals_proc::TypeId::of::<$in_collection>(),
                                    <$in_collection as $crate::macro_internals_proc::ProcCollectionPrivate>::DEF_LOC,
                                ),
                                /* my_path: */ $crate::macro_internals_proc::concat!(
                                    $crate::macro_internals_proc::file!(),
                                    ":",
                                    $crate::macro_internals_proc::line!(),
                                    ":",
                                    $crate::macro_internals_proc::column!(),
                                ),
                                /* borrows: */ <dyn ProcAccess as $crate::macro_internals_proc::IterableAccessTrait>::iter_access(),
                                /* calls: */ [$((
                                    $crate::macro_internals_proc::TypeId::of::<$out_collection>(),
                                    <$out_collection as $crate::macro_internals_proc::ProcCollectionPrivate>::DEF_LOC,
                                )),*],
                            );
                        };
                    }
                }

                // Fetch the tokens
                fn get_token<'a, C>(_input: &'a mut $crate::macro_internals_proc::CanCallCollectionTyProof<'_, C>) -> (
                    &'a mut impl ProcAccess,
                    &'a mut $crate::macro_internals_proc::call_cx![$($out_collection),*],
                )
                where
                    C: $crate::macro_internals_proc::ProcCollection,
                {
                    (
                        $crate::macro_internals_proc::DangerousGlobalAccessToken::new(),
                        $crate::macro_internals_proc::DangerousGlobalAccessToken::new(),
                    )
                }

                get_token(&mut __input)
            };

            $($body)*
        })*
    };
}

// === Validation === //

mod validator;

cfgenius::cond! {
    if not(macro(IS_STATIC_VALIDATION_DISABLED)) {
        impl validator::Validator {
            pub fn global() -> Self {
                let mut validator = validator::Validator::default();

                for proc_register in proc::PROCEDURE_REGISTRARS {
                    proc_register(&mut validator);
                }

                validator
            }
        }
    }
}

pub fn validate() -> Result<(), String> {
    cfgenius::cond_expr! {
        if macro(IS_STATIC_VALIDATION_DISABLED) {
            Ok(())
        } else {
            use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

            static HAS_VALIDATED: AtomicBool = AtomicBool::new(false);

            if !HAS_VALIDATED.load(Relaxed) {
                validator::Validator::global().validate()?;
                HAS_VALIDATED.store(true, Relaxed);
            }

            Ok(())
        }
    }
}

pub struct RootCollectionCallToken<C: ProcCollection> {
    _private: PhantomData<C>,
}

impl<C: ProcCollection> can_call_collection::Private<C> for RootCollectionCallToken<C> {}

impl<C: ProcCollection> CanCallCollection<C> for RootCollectionCallToken<C> {
    fn as_dyn(&self) -> &dyn CanCallCollection<C> {
        DangerousGlobalAccessToken::new()
    }

    fn as_dyn_mut(&mut self) -> &mut dyn CanCallCollection<C> {
        DangerousGlobalAccessToken::new()
    }
}

impl<C: ProcCollection> RootCollectionCallToken<C> {
    // TODO: Enforce overlap rules
    pub fn acquire() -> Self {
        if let Err(err) = validate() {
            panic!("{err}");
        }

        Self {
            _private: PhantomData,
        }
    }
}

impl<C: ProcCollection> Drop for RootCollectionCallToken<C> {
    fn drop(&mut self) {
        // (no-op for now, kept for forwards compatibility)
    }
}
