// Copyright 2026 The Chromium Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::{cell::UnsafeCell, ptr::NonNull};

use allocative::Allocative;
use starlark::values::ProvidesStaticType;
use types::{LabelRef, PackageRef, PathResolver, TargetRef as _};

use crate::{errors::Error, Scope};

enum EvalContextKind {
    BzlFile,
    Macro {
        scope: NonNull<Scope>,
        err: NonNull<crate::bridge::Err>,
    },
    RuleImpl {
        state: UnsafeCell<types::CtxState<crate::TargetRef>>,
        err: NonNull<crate::bridge::Err>,
    },
}

#[derive(Allocative, ProvidesStaticType)]
pub struct EvalContext {
    #[allocative(skip)]
    session: &'static crate::session::Session,
    #[allocative(skip)]
    package: &'static PackageRef,
    #[allocative(skip)]
    kind: EvalContextKind,
}

impl EvalContext {
    /// Creates a new `EvalContext` for evaluating a .bzl file.
    pub fn new_bzl_file(
        session: &'static crate::session::Session,
        package: &'static PackageRef,
    ) -> Self {
        Self {
            session,
            package,
            kind: EvalContextKind::BzlFile,
        }
    }

    /// Creates a new `EvalContext` for macro evaluation within a GN scope.
    pub fn new_macro(
        session: &'static crate::session::Session,
        scope: NonNull<Scope>,
        err: NonNull<crate::bridge::Err>,
    ) -> Self {
        // Safety: The Scope pointer is valid and non-null for the duration of macro
        // evaluation.
        let scope_ref = unsafe { scope.as_ref() };
        // Safety: Package paths in GN scopes are interned and valid for the build
        // session.
        let package: &'static types::PackageRef =
            unsafe { types::util::extend_lifetime(scope_ref.package()) };

        Self {
            session,
            package,
            kind: EvalContextKind::Macro { scope, err },
        }
    }

    /// Creates a new `EvalContext` for rule implementation execution.
    pub fn new_rule_impl(
        session: &'static crate::session::Session,
        target: crate::TargetRef,
        err: NonNull<crate::bridge::Err>,
    ) -> Self {
        let target_package = target.0.label().package();
        // Safety: Package paths in GN scopes/targets are interned and valid for the
        // build session.
        let package: &'static types::PackageRef =
            unsafe { types::util::extend_lifetime(target_package) };

        Self {
            session,
            package,
            kind: EvalContextKind::RuleImpl {
                state: UnsafeCell::new(types::CtxState::new(target)),
                err,
            },
        }
    }

    /// Returns the error pointer associated with this context, if any.
    pub fn err(&self) -> Option<NonNull<crate::bridge::Err>> {
        match &self.kind {
            EvalContextKind::Macro { err, .. } | EvalContextKind::RuleImpl { err, .. } => {
                Some(*err)
            },
            EvalContextKind::BzlFile => None,
        }
    }
}

impl types::EvalContext for EvalContext {
    type Scope = crate::Scope;
    type Session = crate::session::Session;

    fn current_package(&self) -> &types::PackageRef {
        self.package
    }

    fn path_resolver(&self) -> &PathResolver {
        &self.session.path_resolver
    }

    fn session(&self) -> &Self::Session {
        self.session
    }

    fn current_toolchain(&self) -> LabelRef<'_> {
        match &self.kind {
            EvalContextKind::Macro { scope, .. } => {
                // Safety: The eval context is single-threaded, so we cannot have multiple
                // references at the same time.
                unsafe { scope.as_ref() }.settings().toolchain()
            },
            EvalContextKind::BzlFile => {
                unreachable!("There is no current file during bzl file evaluation")
            },
            EvalContextKind::RuleImpl { .. } => {
                self.require_rule_impl().unwrap().target.toolchain()
            },
        }
    }

    // EvalContext uses interior mutability to provide access to the mutable scope.
    #[allow(clippy::mut_from_ref)]
    fn require_macro(&self) -> starlark::Result<std::pin::Pin<&mut Self::Scope>> {
        match &self.kind {
            EvalContextKind::Macro { mut scope, .. } => {
                // Safety: The eval context is single-threaded, so we cannot have multiple
                // references at the same time.
                Ok(unsafe { std::pin::Pin::new_unchecked(scope.as_mut()) })
            },
            _ => Err(Error::RequiresMacro.into()),
        }
    }

    fn require_bzl(&self) -> starlark::Result<()> {
        matches!(self.kind, EvalContextKind::BzlFile)
            .then_some(())
            .ok_or_else(|| Error::RequiresBzlFile.into())
    }

    // EvalContext uses interior mutability to provide access to the mutable state.
    #[allow(clippy::mut_from_ref)]
    fn require_rule_impl(&self) -> starlark::Result<&mut types::CtxState<crate::TargetRef>> {
        match &self.kind {
            EvalContextKind::RuleImpl { state, .. } => {
                // Safety: The eval context is single-threaded, so we cannot have multiple
                // references at the same time.
                Ok(unsafe { &mut *state.get() })
            },
            _ => Err(Error::RequiresRuleImpl.into()),
        }
    }
}

impl attr::traits::EvalContextAttrExt for EvalContext {
    fn create_target(
        &self,
        target_type: Option<types::OutputType>,
        target_name: &str,
        scope: std::pin::Pin<&mut Scope>,
    ) -> starlark::Result<
        std::pin::Pin<
            &'static mut <<Self::Session as types::Session>::TargetRef as types::TargetRef>::Cxx,
        >,
    > {
        let output_type = target_type.map_or("noop", |t| t.into());
        let mut err_ptr = match &self.kind {
            EvalContextKind::Macro { err, .. } => *err,
            _ => return Err(Error::RequiresMacro.into()),
        };
        // Safety: err_ptr is valid and pinned for evaluation duration.
        let err_pin = unsafe { std::pin::Pin::new_unchecked(err_ptr.as_mut()) };
        let target_ptr = crate::bridge::create_target(scope, target_name, output_type, err_pin);
        // Safety: err_ptr is valid for evaluation duration.
        unsafe { err_ptr.as_ref() }.into_result()?;
        let mut target_non_null =
            NonNull::new(target_ptr).expect("Target pointer is null but no error was set");
        // Safety: Target created in GN Scope item collector is heap-allocated, pinned,
        // and lives for session.
        Ok(unsafe { std::pin::Pin::new_unchecked(target_non_null.as_mut()) })
    }

    fn register_target(
        &self,
        cxx: &'static <<Self::Session as types::Session>::TargetRef as types::TargetRef>::Cxx,
        rule: starlark::values::FrozenValue,
        attrs: Vec<attr::Attr>,
    ) -> starlark::Result<<Self::Session as types::Session>::TargetRef> {
        let typed_rule =
            starlark::values::FrozenValueTyped::<rule::FrozenRule<Self>>::new_err(rule)?;
        Ok(self.session.register_target(crate::target::Target {
            cxx,
            starlark: typed_rule
                .has_implementation()
                .then(|| crate::target::StarlarkTarget {
                    rule: typed_rule,
                    attrs,
                }),
            providers: Default::default(),
        }))
    }
}

rule::impl_ctx_methods!(EvalContext);
