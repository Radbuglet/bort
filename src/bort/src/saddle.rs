use crate::{
    entity::{CompMut, CompRef, Entity, OwnedEntity},
    obj::{Obj, OwnedObj},
};

// Macro internals
#[doc(hidden)]
pub mod macro_internals_forwards {
    pub use {
        super::{proc as proc_me, BortComponents},
        saddle::{access_cx, proc, proc_collection},
        std::mem::drop,
    };

    pub trait Cons {
        type PushBack<T>;

        fn push_back<T>(self, value: T) -> Self::PushBack<T>;
    }

    impl Cons for () {
        type PushBack<T> = (T, ());

        fn push_back<T>(self, value: T) -> Self::PushBack<T> {
            (value, ())
        }
    }

    impl<V, R: Cons> Cons for (V, R) {
        type PushBack<T> = (V, R::PushBack<T>);

        fn push_back<T>(self, value: T) -> Self::PushBack<T> {
            (self.0, self.1.push_back(value))
        }
    }
}

// Universes
saddle::universe!(pub BortComponents);

// Proc Collections
pub use saddle::ProcCollection;

#[macro_export]
macro_rules! proc_collection {
    ($(
        $(#[$attr:meta])*
        $vis:vis $name:ident
    );* $(;)?) => {
        $crate::saddle::macro_internals_forwards::proc_collection! {$(
            $(#[$attr])*
            $vis $name;
        )*}
    };
    ($(derive for $target:ty$(;)?)*) => {
        $crate::saddle::macro_internals_forwards::proc_collection! {$(
            derive for $target;
        )*}
    };
    (
        args {}

        $(#[$attr_meta:meta])*
        $vis:vis fn $name:ident
            $(
                <$($generic:ident),* $(,)?>
                $(<$($fn_lt:lifetime),* $(,)?>)?
            )?
            (
                $($para_name:ident: $para:ty),* $(,)?
            ) $(-> $ret:ty)?
        $(where $($where_token:tt)*)?
    ) => {
        $crate::saddle::macro_internals_forwards::proc_collection!(derive for $name);
    };
}

pub use proc_collection;

// Collection tokens
pub use saddle::{call_cx, CanCallCollection};

// Access tokens
pub trait AccessRef<T: 'static>: saddle::AccessRef<BortComponents, T> {
    fn as_dyn(&self) -> &dyn AccessRef<T>;

    fn as_dyn_mut(&mut self) -> &mut dyn AccessRef<T>;
}

impl<T: 'static, K: ?Sized + saddle::AccessRef<BortComponents, T>> AccessRef<T> for K {
    fn as_dyn(&self) -> &dyn AccessRef<T> {
        saddle::DangerousGlobalAccessToken::new()
    }

    fn as_dyn_mut(&mut self) -> &mut dyn AccessRef<T> {
        saddle::DangerousGlobalAccessToken::new()
    }
}

pub trait AccessMut<T: 'static>: saddle::AccessMut<BortComponents, T> {
    fn as_dyn(&self) -> &dyn AccessMut<T>;

    fn as_dyn_mut(&mut self) -> &mut dyn AccessMut<T>;
}

impl<T: 'static, K: ?Sized + saddle::AccessMut<BortComponents, T>> AccessMut<T> for K {
    fn as_dyn(&self) -> &dyn AccessMut<T> {
        saddle::DangerousGlobalAccessToken::new()
    }

    fn as_dyn_mut(&mut self) -> &mut dyn AccessMut<T> {
        saddle::DangerousGlobalAccessToken::new()
    }
}

#[macro_export]
macro_rules! access_cx {
    ($(
        $(#[$attr:meta])*
        $vis:vis trait $name:ident$(: $($inherits:path),*$(,)?)? $(=
            $($kw:ident $component:ty),*$(,)?
        )?
        ;
    )*) => {
        $crate::saddle::macro_internals_forwards::access_cx! {$(
            $(#[$attr])*
            $vis trait $name$(: $($inherits),*)? $(= $($kw $component),* : $crate::saddle::macro_internals_forwards::BortComponents)?;
        )*}
    };
    (
        $($kw:ident $component:ty),* $(,)?
        $(; $($inherits:path),*$(,)?)?
    ) => {
        $crate::saddle::macro_internals_forwards::access_cx![
            $($kw $component),* : $crate::saddle::macro_internals_forwards::BortComponents
            $(; $($inherits),*)?
        ]
    };
}

pub use access_cx;

// Alias
#[macro_export]
macro_rules! alias {
    ($($vis:vis let $name:ident: $ty:ty);*$(;)?) => {$(
        #[allow(non_camel_case_types)]
        $vis type $name = $ty;
    )*};
}

pub use alias;

// Proc
#[macro_export]
macro_rules! proc {
    (
        as $in_collection:ty[$in_collection_cx:expr] do
        $(
            (
                $access_cx_name:ident: [
                    $($access_kw:ident $access_component:ty),* $(,)?
                    $(; $($access_inherits:path),* $(,)?)?
                ],
                $collection_cx_name:ident: [
                    $($out_collection:ty),* $(,)?
                ]
                $(,
                    $($alias_kw:ident $alias_ty:ty $(as $alias_rename:ident)? = $alias_target:expr),*
                    $(,)?
                )?
            ) {
                $($body:tt)*
            }
            $(,)?
        )*
    ) => {
        $crate::saddle::macro_internals_forwards::proc! {
            as $in_collection[$in_collection_cx] do
            $(
                (
                    $access_cx_name: [
                        $($access_kw $access_component,)* $($($alias_kw $alias_ty,)*)? : $crate::saddle::macro_internals_forwards::BortComponents
                        $(; $($access_inherits),*)?
                    ],
                    $collection_cx_name: [
                        $($out_collection),*
                    ]
                ) {
                    // Collect borrows
                    let __borrows = ();
                    $($(
                        $crate::saddle::macro_internals_forwards::proc_me!(
                            @fetch_alias __borrows $access_cx_name $alias_target => $alias_kw $alias_ty
                        );
                    )*)?

                    // Reborrow
                    let mut __borrows = __borrows;

                    #[allow(unused)]
                    let __accum = &mut __borrows;
                    $($(
                        $crate::saddle::macro_internals_forwards::proc_me!(
                            @dump_alias __accum $alias_kw [$($alias_rename)? $alias_ty]
                        );
                    )*)?

                    $($body)*
                    $crate::saddle::macro_internals_forwards::drop(__borrows);
                }
            )*
        }
    };
    (@fetch_alias $borrows:ident $access_cx_name:ident $target:expr => ref $ty:ty) => {
        let $borrows = $crate::saddle::macro_internals_forwards::Cons::push_back(
            $borrows,
            $target.get_s::<$ty>($access_cx_name),
        );
    };
    (@fetch_alias $borrows:ident $access_cx_name:ident $target:expr => mut $ty:ty) => {
        let $borrows = $crate::saddle::macro_internals_forwards::Cons::push_back(
            $borrows,
            $target.get_mut_s::<$ty>($access_cx_name),
        );
    };
    (@dump_alias $accum:ident ref [$first:ident $($rest:tt)*]) => {
        let $first = &*$accum.0;
        #[allow(unused)]
        let $accum = &mut $accum.1;
    };
    (@dump_alias $accum:ident mut [$first:ident $($rest:tt)*]) => {
        let $first = &mut *$accum.0;
        #[allow(unused)]
        let $accum = &mut $accum.1;
    };
}

pub use proc;

// Validation
pub use saddle::{validate, DangerousGlobalAccessToken, RootCollectionCallToken};

// `saddle_delegate!`
#[doc(hidden)]
pub mod macro_internals_saddle_delegate {
    pub use {
        super::{call_cx, proc_collection},
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
                    bhv: $crate::saddle::macro_internals_saddle_delegate::BehaviorProvider,
                    call_cx: &mut $crate::saddle::macro_internals_saddle_delegate::call_cx![$name],
                    $($para_name: $para),*
                ) $(-> $ret)?
            as deriving $crate::saddle::macro_internals_saddle_delegate::behavior_kind
            as deriving $crate::saddle::macro_internals_saddle_delegate::behavior_delegate { $($list)? }
            as deriving $crate::saddle::macro_internals_saddle_delegate::proc_collection
            $(as deriving $deriving $({ $($deriving_args)* })? )*
            $(where $($where_token)*)?
        );
    };
}

pub use saddle_delegate;

// Safe method variants
impl Entity {
    #[track_caller]
    pub fn try_get_s<T: 'static>(self, _cx: &access_cx![ref T]) -> Option<CompRef<'_, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<T: 'static>(self, _cx: &access_cx![mut T]) -> Option<CompMut<'_, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s<T: 'static>(self, _cx: &access_cx![ref T]) -> CompRef<'_, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<T: 'static>(self, _cx: &access_cx![mut T]) -> CompMut<'_, T> {
        self.get_mut()
    }
}

impl OwnedEntity {
    #[track_caller]
    pub fn try_get_s<'b, T: 'static>(&self, _cx: &'b access_cx![ref T]) -> Option<CompRef<'b, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<'b, T: 'static>(
        &self,
        _cx: &'b access_cx![mut T],
    ) -> Option<CompMut<'b, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s<'b, T: 'static>(&self, _cx: &'b access_cx![ref T]) -> CompRef<'b, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<'b, T: 'static>(&self, _cx: &'b access_cx![mut T]) -> CompMut<'b, T> {
        self.get_mut()
    }
}

impl<T: 'static> Obj<T> {
    #[track_caller]
    pub fn try_get_s(self, _cx: &access_cx![ref T]) -> Option<CompRef<'_, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s(self, _cx: &access_cx![mut T]) -> Option<CompMut<'_, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s(self, _cx: &access_cx![ref T]) -> CompRef<'_, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s(self, _cx: &access_cx![mut T]) -> CompMut<'_, T> {
        self.get_mut()
    }
}

impl<T: 'static> OwnedObj<T> {
    #[track_caller]
    pub fn try_get_s<'b>(&self, _cx: &'b access_cx![ref T]) -> Option<CompRef<'b, T>> {
        self.try_get()
    }

    #[track_caller]
    pub fn try_get_mut_s<'b>(&self, _cx: &'b access_cx![mut T]) -> Option<CompMut<'b, T>> {
        self.try_get_mut()
    }

    #[track_caller]
    pub fn get_s<'b>(&self, _cx: &'b access_cx![ref T]) -> CompRef<'b, T> {
        self.get()
    }

    #[track_caller]
    pub fn get_mut_s<'b>(&self, _cx: &'b access_cx![mut T]) -> CompMut<'b, T> {
        self.get_mut()
    }
}
