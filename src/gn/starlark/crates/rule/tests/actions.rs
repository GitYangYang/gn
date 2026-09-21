// Copyright 2026 The Chromium Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use std::path::Path;

use anyhow::Context as _;
use rule::run;
use tempfile::tempdir;
use testutils::Assert;
use types::{Label, PackageRef, Session};

#[test]
fn test_actions_suite() {
    let _temp_dir = tempdir().unwrap();
    let mut assert = Assert::new_rule_assert();
    assert.modify_globals(|builder| {
        depset::depset_globals!(builder, testutils::FakeEvalContext);
    });
    assert.load_module("//rules:actions.scl");
    assert.pass(
        r#"
load(
    "//rules:actions.scl",
    "already_generated_rule",
    "duplicate_declare_file_rule",
    "empty_filename_rule",
    "label_rule",
    "never_generated_rule",
    "undeclared_output_rule",
    "write_rule",
)

write_rule(name = "write_target")
label_rule(name = "label_target")
empty_filename_rule(name = "empty_filename_target")
duplicate_declare_file_rule(name = "duplicate_declare_file_target")
already_generated_rule(name = "already_generated_target")
undeclared_output_rule(name = "undeclared_output_target", src = "foo/file.cc")
never_generated_rule(name = "never_generated_target")
"#,
    );

    type OutputFile<'a> = (&'a Path, &'a str);
    type TestCase<'a> = (&'a str, Result<(&'a str, &'a [OutputFile<'a>]), &'a str>);

    let test_cases: &[TestCase<'_>] = &[
        (
            "write_target",
            Ok((
                "obj/write_target/declared.txt",
                &[
                    (
                        Path::new("obj/write_target/declared.txt"),
                        "//:write_target",
                    ),
                    (Path::new("obj/write_target/undeclared.txt"), "undeclared"),
                ],
            )),
        ),
        (
            "label_target",
            Ok((
                "",
                &[(Path::new("obj/label_target/label_target.txt"), "label")],
            )),
        ),
        (
            "empty_filename_target",
            Err("Declared file name cannot be empty"),
        ),
        ("duplicate_declare_file_target", Err("was already declared")),
        (
            "already_generated_target",
            Err("has already been generated"),
        ),
        (
            "undeclared_output_target",
            Err("was not declared by this target"),
        ),
        ("never_generated_target", Err("was never generated")),
    ];

    let context = assert.context();
    for &(target_name, expected) in test_cases {
        let label = Label::new(PackageRef::root().to_owned(), target_name.to_owned());
        let target = context
            .session
            .get_target(label.as_ref(), context.session.default_toolchain.as_ref());
        let session = assert.session();
        let res = run(&target, move |t: &testutils::FakeTargetRef| {
            testutils::FakeEvalContext::rule_impl(session.clone(), t.clone())
        });

        match expected {
            Ok((defaultinfo_phony, files)) => {
                let rooted = res.expect(target_name);
                let providers = providers::Providers::try_from(rooted).expect(target_name);

                if defaultinfo_phony.is_empty() {
                    assert_eq!(providers.outputs_phony, None);
                } else {
                    assert_eq!(
                        providers.outputs_phony.unwrap().as_path(),
                        Path::new(defaultinfo_phony)
                    );
                }

                for &(file_path, expected_content) in files {
                    let content = std::fs::read_to_string(file_path)
                        .context(file_path.display().to_string())
                        .unwrap();
                    assert_eq!(content, expected_content);
                }
            },
            Err(expected_error) => {
                let err = res.expect_err(target_name);
                assert!(
                    err.to_string().contains(expected_error),
                    "Target '{target_name}' error '{err}' did not contain expected substring '{expected_error}'"
                );
            },
        }
    }
}
