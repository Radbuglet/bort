use std::{any::type_name, fmt, marker::PhantomData};

use crate::{
    behavior::BehaviorKind,
    core::cell::{OptRef, OptRefMut},
    entity::{CompMut, CompRef, Entity, OwnedEntity},
    event::{EventGroup, EventGroupMarkerWithSeparated},
    obj::{Obj, OwnedObj},
};

// === `alias!` === //

#[macro_export]
macro_rules! alias {
    ($($vis:vis let $name:ident: $ty:ty);*$(;)?) => {$(
        #[allow(non_camel_case_types)]
        $vis type $name = $ty;
    )*};
}

pub use alias;

// === `scope!` === //

pub use saddle::Scope;

#[doc(hidden)]
pub mod scope_macro_internals {
    use {
        super::cx_value_sealed::AccessToken,
        crate::entity::{CompMut, CompRef},
        saddle::Scope,
    };

    pub use {
        super::{scope, Cx},
        partial_scope::partial_shadow,
        saddle::scope as raw_scope,
        std::{mem::drop, option::Option},
    };

    pub fn bind_scope_lifetime<'a, S: Scope>(scope: &'a mut S) -> (&'a mut S, CompLtLimiter<'a>) {
        (scope, CompLtLimiter { _ty: [] })
    }

    pub struct CompLtLimiter<'a> {
        _ty: [&'a (); 0],
    }

    impl<'a> CompLtLimiter<'a> {
        pub fn limit_ref_and_decl<T: 'static>(
            &self,
            s: &impl Scope,
            r: CompRef<'static, T>,
        ) -> CompRef<'a, T> {
            s.decl_dep_ref::<AccessToken<T>>();
            r
        }

        pub fn limit_mut_and_decl<T: 'static>(
            &self,
            s: &impl Scope,
            r: CompMut<'static, T>,
        ) -> CompMut<'a, T> {
            s.decl_dep_mut::<AccessToken<T>>();
            r
        }
    }
}

#[macro_export]
macro_rules! scope {
    // Direct forwards
    (
        $(
            $(#[$attr:meta])*
            $vis:vis $name:ident $(<$($generic:ident),*$(,)?>)?
			$(where {$($where:tt)*})?
        );*
        $(;)?
    ) => {
		$crate::saddle::scope_macro_internals::raw_scope! {
			$(
				$(#[$attr])*
				$vis $name $(<$($generic),*>)?
				$(where {$($where)*})?
			)*
		}
	};

    (use $from:expr $(, inherits $($grant_kw:ident $grant_ty:ty),*$(,)?)?) => {
		$crate::saddle::scope_macro_internals::raw_scope! {
			use $from $(, inherits $($grant_kw $grant_ty),*)?
		}
	};

    // Custom unscoped
    (
        use let $from:expr => $to:ident
			$(, inherits [$($grant_kw:ident $grant_ty:ty),*])?
			$(, access $cx_name:ident: $cx_ty:ty)?
			$(, inject {
				$($direct_kw:ident $direct_name:ident $(as $direct_ty:ty)? = $direct_expr:expr),*
				$(,)?
			} as $inject_as:ident)?
			$(,)?
    ) => {
		// Define a new saddle scope
		let $to = $crate::saddle::scope_macro_internals::raw_scope! {
			use $from $(, inherits $($grant_kw $grant_ty),*)?
		};

		// Bind a lifetime limiter
		let ($to, lt_limiter) = $crate::saddle::scope_macro_internals::bind_scope_lifetime($to);
		let _ = &lt_limiter;

		// Construct a context
		$(let ($to, $cx_name): (_, $cx_ty) = $crate::saddle::scope_macro_internals::Cx::new($to);)?

		// Acquire all our components
		$(
			// Define a structure to contain all our borrowed objects.
			#[allow(unused_mut)]
			let mut $inject_as = {
				#[allow(non_camel_case_types)]
				struct Borrows<$($direct_name),*> {
					$($direct_name: $crate::saddle::scope_macro_internals::Option<$direct_name>),*
				}

				Borrows {
					$($direct_name: $crate::saddle::scope_macro_internals::Option::None),*
				}
			};

			// Add components to it
			$(
				$crate::saddle::scope_macro_internals::scope! {
					@__acquire_cx
						$inject_as lt_limiter $to
						$direct_kw $direct_name [$($direct_ty)? $direct_name] = $direct_expr
				};
			)*
		)?
	};
	(
        use let $from:expr => $to:ident
			$(, inherits [$($grant_kw:ident $grant_ty:ty),*])?
			$(, access $cx_name:ident: $cx_ty:ty)?
			$(, inject {
				$($direct_kw:ident $direct_name:ident $(as $direct_ty:ty)? = $direct_expr:expr),*
				$(,)?
			})?
			$(,)?
    ) => {
		$crate::saddle::scope_macro_internals::scope! {
			use let $from => $to
				$(, inherits [$($grant_kw $grant_ty),*])?
				$(, access $cx_name: $cx_ty)?
				$(, inject {
					$($direct_kw $direct_name $(as $direct_ty)? = $direct_expr),*
				} as injected_bundle)?
		}
	};
	(
        use let $from_and_to:ident
			$(, inherits [$($grant_kw:ident $grant_ty:ty),*])?
			$(, access $cx_name:ident: $cx_ty:ty)?
			$(, inject {
				$($direct_kw:ident $direct_name:ident $(as $direct_ty:ty)? = $direct_expr:expr),*
				$(,)?
			} $(as $inject_as:ident)?)?
			$(,)?
    ) => {
		$crate::saddle::scope_macro_internals::scope! {
			use let $from_and_to => $from_and_to
				$(, inherits [$($grant_kw $grant_ty),*])?
				$(, access $cx_name: $cx_ty)?
				$(, inject {
					$($direct_kw $direct_name $(as $direct_ty)? = $direct_expr),*
				} $(as $inject_as)?)?
		}
	};

    // Custom scoped
    (
        use $from:expr => $to:ident
			$(, inherits [$($grant_kw:ident $grant_ty:ty),*])?
			$(, access $cx_name:ident: $cx_ty:ty)?
			$(, inject {
				$($direct_kw:ident $direct_name:ident $(as $direct_ty:ty)? = $direct_expr:expr),*
				$(,)?
			} as $inject_as:ident)?
			$(,)?
		: $($body:tt)*
    ) => {
		// Declare variables we'll be smuggling into the semi-open scope.
		let __internal_to;
		$(let __internal_cx; { let $cx_name = (); let _ = $cx_name; })?
		$(let __internal_inject; { let $inject_as = (); let _ = $inject_as; })?

		// Acquire them.
		{
			$crate::saddle::scope_macro_internals::scope!(
				use let $from => $to
					$(, inherits [$($grant_kw $grant_ty),*])?
					$(, access $cx_name: $cx_ty)?
					$(, inject {
						$($direct_kw $direct_name $(as $direct_ty)? = $direct_expr),*
					} as $inject_as)?
			);

			__internal_to = $to;
			$(__internal_cx = $cx_name;)?
			$(__internal_inject = $inject_as;)?
		}

		// Define the semi-open scope.
		$crate::saddle::scope_macro_internals::partial_shadow! {
			// Bring them in!
			$to $(, $cx_name)? $(, $inject_as $(, $direct_name)*)?;

			#[allow(unused)]
			let $to = __internal_to;

			$(
				#[allow(unused)]
				let $cx_name = __internal_cx;
			)?
			$(
				#[allow(unused_mut)]
				let mut $inject_as = __internal_inject;

				$($crate::saddle::scope_macro_internals::scope!(
					@__get_out_cx $inject_as $direct_kw $direct_name
				);)*
			)?

			// Paste the body.
			$($body)*

			// Drop the injection context if need be.
			$($crate::saddle::scope_macro_internals::drop($inject_as);)?
		}
	};
	(
        use $from:expr => $to:ident
			$(, inherits [$($grant_kw:ident $grant_ty:ty),*])?
			$(, access $cx_name:ident: $cx_ty:ty)?
			$(, inject {
				$($direct_kw:ident $direct_name:ident $(as $direct_ty:ty)? = $direct_expr:expr),*
				$(,)?
			})?
			$(,)?
		: $($body:tt)*
    ) => {
		$crate::saddle::scope_macro_internals::scope!(
			use $from => $to
				$(, inherits [$($grant_kw $grant_ty),*])?
				$(, access $cx_name: $cx_ty)?
				$(, inject {
					$($direct_kw $direct_name $(as $direct_ty)? = $direct_expr),*
				} as __internal_injected_bundle)?
			: $($body)*
		);
	};
	(
        use $from_and_to:ident
			$(, inherits [$($grant_kw:ident $grant_ty:ty),*])?
			$(, access $cx_name:ident: $cx_ty:ty)?
			$(, inject {
				$($direct_kw:ident $direct_name:ident $(as $direct_ty:ty)? = $direct_expr:expr),*
				$(,)?
			} $(as $inject_as:ident)?)?
			$(,)?
		: $($body:tt)*
    ) => {
		$crate::saddle::scope_macro_internals::scope!(
			use $from_and_to => $from_and_to
				$(, inherits [$($grant_kw $grant_ty),*])?
				$(, access $cx_name: $cx_ty)?
				$(, inject {
					$($direct_kw $direct_name $(as $direct_ty)? = $direct_expr),*
				} $(as $inject_as)?)?
			: $($body)*
		);
	};

	// Internals
	(@__acquire_cx $inject_as:ident $limiter:ident $scope:ident ref $direct_name:ident [$first_ty:ty $(, $remaining_ty:ty)*] = $from:expr) => {
		$inject_as.$direct_name = $crate::saddle::scope_macro_internals::Option::Some(
			$limiter.limit_ref_and_decl($scope, $from.get::<$first_ty>())
		);
		#[allow(unused)]
		let $direct_name = &**$inject_as.$direct_name.as_ref().unwrap();
	};
	(@__acquire_cx $inject_as:ident $limiter:ident $scope:ident mut $direct_name:ident [$first_ty:ty $(, $remaining_ty:ty)*] = $from:expr) => {
		$inject_as.$direct_name = $crate::saddle::scope_macro_internals::Option::Some(
			$limiter.limit_mut_and_decl($scope, $from.get_mut::<$first_ty>())
		);
		#[allow(unused)]
		let $direct_name = &mut **$inject_as.$direct_name.as_mut().unwrap();
	};
	(@__get_out_cx $inject_as:ident ref $direct_name:ident) => {
		#[allow(unused)]
		let $direct_name = &**$inject_as.$direct_name.as_ref().unwrap();
	};
	(@__get_out_cx $inject_as:ident mut $direct_name:ident) => {
		#[allow(unused)]
		let $direct_name = &mut **$inject_as.$direct_name.as_mut().unwrap();
	};
}

pub use scope;

// === `saddle_delegate!` === //

scope! {
    pub BehaviorKindScope<Bhv>
    where { Bhv: BehaviorKind }
}

#[doc(hidden)]
pub mod macro_internals_saddle_delegate {
    pub use {
        super::BehaviorKindScope,
        crate::behavior::{behavior_delegate, behavior_kind, delegate, BehaviorProvider},
    };
}

#[macro_export]
macro_rules! saddle_delegate {
    (
        $(#[$attr_meta:meta])*
        $vis:vis fn $name:ident
            $(
                <$($generic:ident),* $(,)?>
                $(<$($fn_lt:lifetime),* $(,)?>)?
            )?
            ($($para_name:ident: $para:ty),* $(,)?) $(-> $ret:ty)?
		$(as list $list:ty)?
        $(as deriving $deriving:path $({ $($deriving_args:tt)* })? )*
        $(where $($where_token:tt)*)?
    ) => {
        $crate::saddle::macro_internals_saddle_delegate::delegate!(
            $(#[$attr_meta])*
            $vis fn $name
                $(
                    <$($generic),*>
                    $(<$($fn_lt),*>)?
                )?
                (
                    bhv: $crate::saddle::macro_internals_saddle_delegate::BehaviorProvider<'_>,
                    call_cx: &mut $crate::saddle::macro_internals_saddle_delegate::BehaviorKindScope<$name>,
                    $($para_name: $para),*
                ) $(-> $ret)?
            as deriving $crate::saddle::macro_internals_saddle_delegate::behavior_kind
            as deriving $crate::saddle::macro_internals_saddle_delegate::behavior_delegate { $($list)? }
            $(as deriving $deriving $({ $($deriving_args)* })? )*
            $(where $($where_token)*)?
        );
    };
}

pub use saddle_delegate;

// === CxValue === //

mod cx_value_sealed {
    use std::{fmt, marker::PhantomData};

    use saddle::Scope;

    pub struct AccessToken<T: 'static>(pub PhantomData<fn() -> T>);

    pub trait Sealed {
        const SHOULD_FMT: bool;

        type Storage;

        fn new() -> Self::Storage;

        fn fmt_value(f: &mut fmt::Formatter) -> fmt::Result;

        fn decl_borrows(s: &impl Scope);

        fn decl_grants(s: &impl Scope);
    }
}

pub trait CxValue: cx_value_sealed::Sealed {}

pub trait CxValueLt<'a>: CxValue {}

impl cx_value_sealed::Sealed for NoValue {
    const SHOULD_FMT: bool = false;

    type Storage = NoValue;

    fn new() -> Self::Storage {
        NoValue
    }

    fn fmt_value(_f: &mut std::fmt::Formatter) -> std::fmt::Result {
        Ok(())
    }

    fn decl_borrows(s: &impl Scope) {
        let _ = s;
    }

    fn decl_grants(s: &impl Scope) {
        let _ = s;
    }
}

impl CxValue for NoValue {}

impl<'a> CxValueLt<'a> for NoValue {}

impl<'a, T: 'static> cx_value_sealed::Sealed for &'a T {
    const SHOULD_FMT: bool = true;

    type Storage = &'a cx_value_sealed::AccessToken<T>;

    fn new() -> Self::Storage {
        &cx_value_sealed::AccessToken(PhantomData)
    }

    fn fmt_value(f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "&{}", type_name::<T>())
    }

    fn decl_borrows(s: &impl Scope) {
        s.decl_dep_ref::<cx_value_sealed::AccessToken<T>>();
    }

    fn decl_grants(s: &impl Scope) {
        s.decl_grant_ref::<cx_value_sealed::AccessToken<T>>();
    }
}

impl<T: 'static> CxValue for &'_ T {}

impl<'a, T: 'static> CxValueLt<'a> for &'a T {}

impl<'a, T: 'static> cx_value_sealed::Sealed for &'a mut T {
    const SHOULD_FMT: bool = true;

    type Storage = &'a mut cx_value_sealed::AccessToken<T>;

    fn new() -> Self::Storage {
        Box::leak(Box::new(cx_value_sealed::AccessToken(PhantomData)))
    }

    fn fmt_value(f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "&mut {}", type_name::<T>())
    }

    fn decl_borrows(s: &impl Scope) {
        s.decl_dep_mut::<cx_value_sealed::AccessToken<T>>();
    }

    fn decl_grants(s: &impl Scope) {
        s.decl_grant_mut::<cx_value_sealed::AccessToken<T>>();
    }
}

impl<T: 'static> CxValue for &'_ mut T {}

impl<'a, T: 'static> CxValueLt<'a> for &'a mut T {}

// === Cx === //

pub use compost2::NoValue;
use NoValue as N;

pub struct Cx<A = N, B = N, C = N, D = N, E = N, F = N, G = N, H = N, I = N, J = N, K = N, L = N>
where
    A: CxValue,
    B: CxValue,
    C: CxValue,
    D: CxValue,
    E: CxValue,
    F: CxValue,
    G: CxValue,
    H: CxValue,
    I: CxValue,
    J: CxValue,
    K: CxValue,
    L: CxValue,
{
    #[doc(hidden)]
    pub macro_internal_usability_nub: (),

    #[doc(hidden)]
    pub macro_internal_compost_cx: compost2::Cx<
        A::Storage,
        B::Storage,
        C::Storage,
        D::Storage,
        E::Storage,
        F::Storage,
        G::Storage,
        H::Storage,
        I::Storage,
        J::Storage,
        K::Storage,
        L::Storage,
    >,
}

impl<'a, A, B, C, D, E, F, G, H, I, J, K, L> Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValueLt<'a>,
    B: CxValueLt<'a>,
    C: CxValueLt<'a>,
    D: CxValueLt<'a>,
    E: CxValueLt<'a>,
    F: CxValueLt<'a>,
    G: CxValueLt<'a>,
    H: CxValueLt<'a>,
    I: CxValueLt<'a>,
    J: CxValueLt<'a>,
    K: CxValueLt<'a>,
    L: CxValueLt<'a>,
{
    pub fn new<S: Scope>(scope: &'a mut S) -> (&'a mut S, Self) {
        Self::decl_borrows(scope);
        return (scope, Self::new_unchecked());
    }
}

impl<A, B, C, D, E, F, G, H, I, J, K, L> Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValue,
    B: CxValue,
    C: CxValue,
    D: CxValue,
    E: CxValue,
    F: CxValue,
    G: CxValue,
    H: CxValue,
    I: CxValue,
    J: CxValue,
    K: CxValue,
    L: CxValue,
{
    pub fn new_unchecked() -> Self {
        Self {
            macro_internal_usability_nub: (),
            macro_internal_compost_cx: compost2::Cx::from((
                A::new(),
                B::new(),
                C::new(),
                D::new(),
                E::new(),
                F::new(),
                G::new(),
                H::new(),
                I::new(),
                J::new(),
                K::new(),
                L::new(),
            )),
        }
    }

    pub fn decl_borrows(s: &impl Scope) {
        A::decl_borrows(s);
        B::decl_borrows(s);
        C::decl_borrows(s);
        D::decl_borrows(s);
        E::decl_borrows(s);
        F::decl_borrows(s);
        G::decl_borrows(s);
        H::decl_borrows(s);
        I::decl_borrows(s);
        J::decl_borrows(s);
        K::decl_borrows(s);
        L::decl_borrows(s);
    }

    pub fn decl_grants(s: &impl Scope) {
        A::decl_grants(s);
        B::decl_grants(s);
        C::decl_grants(s);
        D::decl_grants(s);
        E::decl_grants(s);
        F::decl_grants(s);
        G::decl_grants(s);
        H::decl_grants(s);
        I::decl_grants(s);
        J::decl_grants(s);
        K::decl_grants(s);
        L::decl_grants(s);
    }
}

impl<A, B, C, D, E, F, G, H, I, J, K, L> fmt::Debug for Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValue,
    B: CxValue,
    C: CxValue,
    D: CxValue,
    E: CxValue,
    F: CxValue,
    G: CxValue,
    H: CxValue,
    I: CxValue,
    J: CxValue,
    K: CxValue,
    L: CxValue,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn fmt_cx<T: CxValue>(f: &mut fmt::DebugTuple) {
            struct FmtCxValue<T>([T; 0]);

            impl<T: CxValue> fmt::Debug for FmtCxValue<T> {
                fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                    T::fmt_value(f)
                }
            }

            if T::SHOULD_FMT {
                f.field(&FmtCxValue::<T>([]));
            }
        }

        let mut f = f.debug_tuple("Cx");
        fmt_cx::<A>(&mut f);
        fmt_cx::<B>(&mut f);
        fmt_cx::<C>(&mut f);
        fmt_cx::<D>(&mut f);
        fmt_cx::<E>(&mut f);
        fmt_cx::<F>(&mut f);
        fmt_cx::<G>(&mut f);
        fmt_cx::<H>(&mut f);
        fmt_cx::<I>(&mut f);
        fmt_cx::<J>(&mut f);
        fmt_cx::<K>(&mut f);
        fmt_cx::<L>(&mut f);

        f.finish()
    }
}

mod any_cx_sealed {
    pub trait Sealed {}
}

pub trait AnyCx: fmt::Debug + any_cx_sealed::Sealed {
    fn new_unchecked() -> Self;

    fn decl_borrows(s: &impl Scope);

    fn decl_grants(s: &impl Scope);
}

pub trait AnyCxLt<'a>: AnyCx {}

impl<A, B, C, D, E, F, G, H, I, J, K, L> any_cx_sealed::Sealed
    for Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValue,
    B: CxValue,
    C: CxValue,
    D: CxValue,
    E: CxValue,
    F: CxValue,
    G: CxValue,
    H: CxValue,
    I: CxValue,
    J: CxValue,
    K: CxValue,
    L: CxValue,
{
}

impl<A, B, C, D, E, F, G, H, I, J, K, L> AnyCx for Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValue,
    B: CxValue,
    C: CxValue,
    D: CxValue,
    E: CxValue,
    F: CxValue,
    G: CxValue,
    H: CxValue,
    I: CxValue,
    J: CxValue,
    K: CxValue,
    L: CxValue,
{
    fn new_unchecked() -> Self {
        Self::new_unchecked()
    }

    fn decl_borrows(s: &impl Scope) {
        Self::decl_borrows(s);
    }

    fn decl_grants(s: &impl Scope) {
        Self::decl_grants(s);
    }
}

impl<'a, A, B, C, D, E, F, G, H, I, J, K, L> AnyCxLt<'a> for Cx<A, B, C, D, E, F, G, H, I, J, K, L>
where
    A: CxValueLt<'a>,
    B: CxValueLt<'a>,
    C: CxValueLt<'a>,
    D: CxValueLt<'a>,
    E: CxValueLt<'a>,
    F: CxValueLt<'a>,
    G: CxValueLt<'a>,
    H: CxValueLt<'a>,
    I: CxValueLt<'a>,
    J: CxValueLt<'a>,
    K: CxValueLt<'a>,
    L: CxValueLt<'a>,
{
}

// === `cx!` macro === //

#[doc(hidden)]
pub mod cx_macro_internals {
    use super::CxValue;

    pub use {super::Cx, compost2::cx as compost_cx, std::marker::PhantomData};

    pub fn ensure_is_context<A, B, C, D, E, F, G, H, I, J, K, L>(
        _cx: &Cx<A, B, C, D, E, F, G, H, I, J, K, L>,
        _bound: &PhantomData<Cx<A, B, C, D, E, F, G, H, I, J, K, L>>,
    ) -> !
    where
        A: CxValue,
        B: CxValue,
        C: CxValue,
        D: CxValue,
        E: CxValue,
        F: CxValue,
        G: CxValue,
        H: CxValue,
        I: CxValue,
        J: CxValue,
        K: CxValue,
        L: CxValue,
    {
        unreachable!();
    }

    pub fn bind_marker<T>(_bound: &PhantomData<T>, value: T) -> T {
        value
    }
}

#[macro_export]
macro_rules! cx {
    (noalias $target:ident) => {{
		let binder = $crate::saddle::cx_macro_internals::PhantomData;

		#[allow(unreachable_code)]
        if false {
			loop {}  // This is necessary in case droppable objects depend on borrows of $target.
            $crate::saddle::cx_macro_internals::ensure_is_context(&$target, &binder);
        }
		let _ = $target.macro_internal_usability_nub;

		let duplicate = $crate::saddle::cx_macro_internals::bind_marker(
			&binder,
			$crate::saddle::cx_macro_internals::Cx::new_unchecked(),
		);

		$crate::saddle::cx_macro_internals::Cx {
			macro_internal_usability_nub: (),
			macro_internal_compost_cx: $crate::saddle::cx_macro_internals::compost_cx!(
				dangerously_specify_place { duplicate.macro_internal_compost_cx },
			),
		}
    }};
	($target:ident) => {{
		#[allow(unreachable_code)]
        if false {
			loop {}  // This is necessary in case droppable objects depend on borrows of $target.
            $crate::saddle::cx_macro_internals::ensure_is_context(
				&$target,
				&$crate::saddle::cx_macro_internals::PhantomData,
			);
        }

		let _ = $target.macro_internal_usability_nub;

		$crate::saddle::cx_macro_internals::Cx {
			macro_internal_usability_nub: (),
			macro_internal_compost_cx: $crate::saddle::cx_macro_internals::compost_cx!(
				dangerously_specify_place { $target.macro_internal_compost_cx },
			),
		}
    }};
}

pub use cx;

// === Safe Variants === //

impl Entity {
    #[track_caller]
    pub fn try_get_s<T: 'static>(self, _cx: Cx<&T>) -> Option<CompRef<'_, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<T: 'static>(self, _cx: Cx<&mut T>) -> Option<CompMut<'_, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s<T: 'static>(self, _cx: Cx<&T>) -> CompRef<'_, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<T: 'static>(self, _cx: Cx<&mut T>) -> CompMut<'_, T> {
        self.get_mut()
    }
}

impl OwnedEntity {
    #[track_caller]
    pub fn try_get_s<'b, T: 'static>(&self, _cx: Cx<&'b T>) -> Option<CompRef<'b, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<'b, T: 'static>(&self, _cx: Cx<&'b mut T>) -> Option<CompMut<'b, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s<'b, T: 'static>(&self, _cx: Cx<&'b T>) -> CompRef<'b, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<'b, T: 'static>(&self, _cx: Cx<&'b mut T>) -> CompMut<'b, T> {
        self.get_mut()
    }
}

impl<T: 'static> Obj<T> {
    #[track_caller]
    pub fn try_get_s(self, _cx: Cx<&T>) -> Option<CompRef<'_, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s(self, _cx: Cx<&mut T>) -> Option<CompMut<'_, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s(self, _cx: Cx<&T>) -> CompRef<'_, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s(self, _cx: Cx<&mut T>) -> CompMut<'_, T> {
        self.get_mut()
    }
}

impl<T: 'static> OwnedObj<T> {
    #[track_caller]
    pub fn try_get_s<'b>(&self, _cx: Cx<&'b T>) -> Option<CompRef<'b, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<'b>(&self, _cx: Cx<&'b mut T>) -> Option<CompMut<'b, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s<'b>(&self, _cx: Cx<&'b T>) -> CompRef<'b, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<'b>(&self, _cx: Cx<&'b mut T>) -> CompMut<'b, T> {
        self.get_mut()
    }
}

impl<G: ?Sized> EventGroup<G> {
    pub fn get_s<'a, E>(&'a self, _cx: Cx<&'a G::List>) -> OptRef<'a, G::List>
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        self.get::<E>()
    }

    pub fn get_mut_s<'a, E>(&'a self, _cx: Cx<&'a mut G::List>) -> OptRefMut<'_, G::List>
    where
        G: EventGroupMarkerWithSeparated<E>,
    {
        self.get_mut::<E>()
    }
}

// === ScopeExt === //

pub trait ScopeExt {
    fn call_with_grant<'a, C: AnyCxLt<'a>, S: Scope>(&'a mut self, cx: C) -> &'a mut S;
}

impl<SParent: Scope> ScopeExt for SParent {
    fn call_with_grant<'a, C: AnyCxLt<'a>, SChild: Scope>(&'a mut self, cx: C) -> &'a mut SChild {
        let _ = cx;

        scope!(SubScope<SParent, SChild>);

        let sub_scope = self.decl_call::<SubScope<SParent, SChild>>();
        C::decl_grants(sub_scope);
        sub_scope.decl_call()
    }
}
