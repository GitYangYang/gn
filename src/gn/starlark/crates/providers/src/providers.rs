// Copyright 2026 The Chromium Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use args::FrozenArgsSequence;
use depset::UnpackFileDepset;
use starlark::{
    collections::SmallMap,
    typing::Ty,
    values::{
        list::ListRef, structs::StructRef, type_repr::StarlarkTypeRepr, OwnedFrozen,
        ProvidesStaticType, UnpackValue as _, Value,
    },
};
use types::File;

use crate::{ProviderId, ProviderInstance};

/// The unpacked metadata from a target's providers.
#[derive(Debug, Default, ProvidesStaticType)]
struct InnerProviders<'v> {
    /// Command-line substitution variables propagated by this target, parsed
    /// from the `GnSubstitutionsInfo` provider.
    ///
    /// `GnSubstitutionsInfo` carries key-value substitutions (packaged as a
    /// struct) that are used by toolchains and command-line execution
    /// blocks to expand variables and flags dynamically.
    substitutions: SmallMap<String, FrozenArgsSequence<'v>>,

    /// All provider instances mapped by their ProviderType's ProviderId.
    value: SmallMap<ProviderId, Value<'v>>,
}

/// Helper to unpack a frozen list of providers to useful metadata.
#[derive(Debug)]
pub struct Providers {
    /// The output files produced by this target, parsed from the `DefaultInfo`
    /// provider.
    ///
    /// `DefaultInfo` represents the default outputs of a target rule (similar
    /// to Bazel's `DefaultInfo`). In GN, it contains the `files` depset
    /// which lists the direct and transitive outputs of the rule.
    pub outputs_phony: Option<File>,

    /// Additional input files declared by this target, parsed from the
    /// `GnInputsInfo` provider.
    ///
    /// `GnInputsInfo` is a built-in provider used by target rules to declare
    /// additional, dynamic input files that the rule's tool dependencies or
    /// parent targets must track.
    pub inputs_phony: Option<File>,

    owned: OwnedFrozen<InnerProviders<'static>>,
}

impl Providers {
    /// Inspect the inner provider instances and substitutions within a scoped
    /// closure.
    pub fn by_ref<'s, R>(
        &'s self,
        f: impl for<'a, 'v> FnOnce(
            &'a SmallMap<String, FrozenArgsSequence<'v>>,
            &'a SmallMap<ProviderId, Value<'v>>,
        ) -> R,
    ) -> R {
        self.owned
            .by_ref(|inner| f(&inner.substitutions, &inner.value))
    }
}

impl StarlarkTypeRepr for Providers {
    type Canonical = Value<'static>;

    fn starlark_type_repr() -> Ty {
        Ty::list(Ty::any())
    }
}

impl TryFrom<OwnedFrozen<Value<'static>>> for Providers {
    type Error = starlark::Error;

    fn try_from(value: OwnedFrozen<Value<'static>>) -> Result<Self, Self::Error> {
        let (inner_res, (outputs_phony, inputs_phony)) = value.try_by_value_with_reconstructor(
            |val, _r| {
                let list = match <&ListRef>::unpack_value_err(val) {
                    Ok(l) => l,
                    Err(e) => return (Err(e), (None, None)),
                };
                let mut value_map = SmallMap::new();
                let mut outputs_phony = None;
                let mut inputs_phony = None;
                let mut substitutions = SmallMap::new();

                for item in list.iter() {
                    let instance = match <&ProviderInstance>::unpack_value_err(item) {
                        Ok(inst) => inst,
                        Err(e) => return (Err(e), (outputs_phony, inputs_phony)),
                    };

                    if value_map.insert(instance.provider_type.id, item).is_some() {
                        return (
                            Err(crate::errors::Error::DuplicateProvider(instance.ty_name().to_owned()).into()),
                            (outputs_phony, inputs_phony),
                        );
                    }

                    match instance.provider_type.id {
                        crate::builtins::DEFAULT_INFO_ID => {
                            let files = instance.values[0].unwrap();
                            match UnpackFileDepset::unpack_value_err(files) {
                                Ok(f) => outputs_phony = f.0,
                                Err(_) => {
                                    return (
                                        Err(crate::errors::Error::DefaultInfoFilesMustBeFileDepset(files.to_repr()).into()),
                                        (outputs_phony, inputs_phony),
                                    );
                                }
                            }
                        }
                        crate::builtins::INPUTS_INFO_ID => {
                            let files = instance.values[0].unwrap();
                            match UnpackFileDepset::unpack_value_err(files) {
                                Ok(f) => inputs_phony = f.0,
                                Err(_) => {
                                    return (
                                        Err(crate::errors::Error::GnInputsInfoFilesMustBeFileDepset(files.to_repr()).into()),
                                        (outputs_phony, inputs_phony),
                                    );
                                }
                            }
                        }
                        crate::builtins::SUBSTITUTIONS_INFO_ID => {
                            // GnSubstitutionsInfo(substitutions = struct)
                            let substitutions_val = instance.values[0].unwrap();
                            let Some(substitutions_struct) = StructRef::from_value(substitutions_val) else {
                                return (
                                    Err(crate::errors::Error::GnSubstitutionsInfoSubstitutionsMustBeStruct(
                                        substitutions_val.to_repr(),
                                    ).into()),
                                    (outputs_phony, inputs_phony),
                                );
                            };

                            for (k, v) in substitutions_struct.iter() {
                                let seq: FrozenArgsSequence = match <FrozenArgsSequence>::unpack_value_err(v) {
                                    Ok(s) => s,
                                    Err(e) => return (Err(e), (outputs_phony, inputs_phony)),
                                };
                                substitutions.insert(k.as_str().to_owned(), seq);
                            }
                        }
                        _ => {}
                    }
                }

                (
                    Ok(InnerProviders {
                        substitutions,
                        value: value_map,
                    }),
                    (outputs_phony, inputs_phony),
                )
            },
        );

        Ok(Self {
            outputs_phony,
            inputs_phony,
            owned: inner_res?,
        })
    }
}

#[cfg(test)]
mod tests {

    use types::File;

    use crate::Providers;

    fn new_assert() -> testutils::Assert {
        let mut a = testutils::Assert::default();

        a.modify_globals(|builder| {
            depset::depset_globals!(builder, testutils::eval_context::FakeEvalContext);
            let builtin_providers = crate::globals::register_providers(builder);
            // Note: In production code, the module would be preloaded instead of loading
            // into globals.
            builder.set(
                "GnInputsInfo",
                builtin_providers.module.get("GnInputsInfo").unwrap(),
            );
            builder.set(
                "GnSubstitutionsInfo",
                builtin_providers.module.get("GnSubstitutionsInfo").unwrap(),
            );
        });
        a
    }

    #[test]
    fn test_providers_unpacking() {
        let mut a = new_assert();

        let val = a.pass("[]");
        let providers = Providers::try_from(val).unwrap();
        assert_eq!(providers.outputs_phony, None);
        assert_eq!(providers.inputs_phony, None);
        providers.by_ref(|substitutions, value| {
            assert!(substitutions.is_empty());
            assert!(value.is_empty());
        });

        let custom_info_ty = a.pass("CustomInfo = provider(fields = ['foo']); CustomInfo");
        a.modify_globals(move |builder| {
            builder.set("CustomInfo", custom_info_ty.clone());
        });

        let val = a.pass(
            r#"[
    DefaultInfo(files = depset([make_file("a")])),
    GnInputsInfo(files = depset([make_file("b")])),
    GnSubstitutionsInfo(substitutions = struct(key = ["val"])),
    CustomInfo(foo = 1),
]"#,
        );
        let providers = Providers::try_from(val).unwrap();

        assert_eq!(providers.outputs_phony, Some(File::intern("a")));
        assert_eq!(providers.inputs_phony, Some(File::intern("b")));

        providers.by_ref(|substitutions, value| {
            let keys: Vec<&str> = substitutions.keys().map(|s| s.as_str()).collect();
            assert_eq!(keys, vec!["key"]);

            assert_eq!(value.len(), 4);
            assert!(value.contains_key(&crate::builtins::DEFAULT_INFO_ID));
            assert!(value.contains_key(&crate::builtins::INPUTS_INFO_ID));
            assert!(value.contains_key(&crate::builtins::SUBSTITUTIONS_INFO_ID));
        });
    }

    #[track_caller]
    fn assert_unpack_fails(a: &mut testutils::Assert, expr: &str, expected_err: &str) {
        let val = a.pass(expr);
        let err = Providers::try_from(val).unwrap_err();
        assert_eq!(err.to_string(), expected_err);
    }

    #[test]
    fn test_providers_unpack_fails() {
        let mut a = new_assert();

        assert_unpack_fails(
            &mut a,
            "[DefaultInfo(files = depset()), DefaultInfo(files = depset())]",
            "Duplicate provider: DefaultInfo",
        );

        assert_unpack_fails(
            &mut a,
            r#"[DefaultInfo(files = "not-a-depset")]"#,
            r#"DefaultInfo.files must be a depset of files, got "not-a-depset""#,
        );

        assert_unpack_fails(
            &mut a,
            r#"[DefaultInfo(files = depset(["not-a-file"]))]"#,
            "DefaultInfo.files must be a depset of files, got depset(...)",
        );

        assert_unpack_fails(
            &mut a,
            r#"[GnInputsInfo(files = "not-a-depset")]"#,
            r#"GnInputsInfo.files must be a depset of files, got "not-a-depset""#,
        );

        assert_unpack_fails(
            &mut a,
            r#"[GnSubstitutionsInfo(substitutions = {"key": ["val"]})]"#,
            r#"GnSubstitutionsInfo.substitutions must be a struct, got {"key": ["val"]}"#,
        );

        assert_unpack_fails(
            &mut a,
            r#"[GnSubstitutionsInfo(substitutions = struct(key = "not-a-list"))]"#,
            r#"Expected `list`, but got `string (repr: "not-a-list")`"#,
        );

        assert_unpack_fails(
            &mut a,
            r#"[GnSubstitutionsInfo(substitutions = struct(key = [123]))]"#,
            "Expected `Args | str`, but got `int (repr: 123)`",
        );
    }
}
