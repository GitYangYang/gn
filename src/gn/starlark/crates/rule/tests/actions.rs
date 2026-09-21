// Copyright 2026 The Chromium Authors. All rights reserved.
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

use rule::run;
use starlark::values::list::ListRef;
use testutils::Assert;
use types::{Label, PackageRef, Session};

struct CleanupToolchainDir;
impl Drop for CleanupToolchainDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all("$TOOLCHAIN");
    }
}

fn new_actions_assert() -> (Assert, CleanupToolchainDir) {
    let mut assert = Assert::new_rule_assert();
    assert.modify_globals(|builder| {
        depset::depset_globals!(builder, testutils::FakeEvalContext);
    });
    assert.load_module("//rules:actions.scl");
    (assert, CleanupToolchainDir)
}

#[test]
fn test_actions_write_action() {
    let (mut assert, _cleanup) = new_actions_assert();

    assert.pass(
        r#"
load("//rules:actions.scl", "write_rule")

write_rule(
    name = "write_target",
)
"#,
    );

    let context = assert.context();
    let label = Label::new(PackageRef::root().to_owned(), "write_target".to_owned());
    let target = context
        .session
        .get_target(label.as_ref(), context.session.default_toolchain.as_ref());

    let session = assert.session();
    let res = run(&target, move |t: &testutils::FakeTargetRef| {
        testutils::FakeEvalContext::rule_impl(session.clone(), t.clone())
    })
    .unwrap();

    let list = ListRef::from_value(res.value()).unwrap();
    let items: Vec<starlark::values::Value<'_>> = list.iter().collect();
    assert_eq!(items.len(), 1);
    assert!(items[0].to_repr().contains("written.txt"));
}

#[test]
fn test_actions_args() {
    let (mut assert, _cleanup) = new_actions_assert();

    assert.pass(
        r#"
load("//rules:actions.scl", "args_rule")

args_rule(
    name = "args_target",
)
"#,
    );

    let context = assert.context();
    let label = Label::new(PackageRef::root().to_owned(), "args_target".to_owned());
    let target = context
        .session
        .get_target(label.as_ref(), context.session.default_toolchain.as_ref());

    let session = assert.session();
    let res = run(&target, move |t: &testutils::FakeTargetRef| {
        testutils::FakeEvalContext::rule_impl(session.clone(), t.clone())
    })
    .unwrap();

    let list = ListRef::from_value(res.value()).unwrap();
    let items: Vec<starlark::values::Value<'_>> = list.iter().collect();
    assert_eq!(items.len(), 1);
    assert!(items[0].to_repr().contains("args_out.txt"));
}

#[test]
fn test_actions_ctx_label() {
    let (mut assert, _cleanup) = new_actions_assert();

    assert.pass(
        r#"
load("//rules:actions.scl", "label_rule")

label_rule(
    name = "label_target",
)
"#,
    );

    let context = assert.context();
    let label = Label::new(PackageRef::root().to_owned(), "label_target".to_owned());
    let target = context
        .session
        .get_target(label.as_ref(), context.session.default_toolchain.as_ref());

    let session = assert.session();
    let res = run(&target, move |t: &testutils::FakeTargetRef| {
        testutils::FakeEvalContext::rule_impl(session.clone(), t.clone())
    })
    .unwrap();

    let list = ListRef::from_value(res.value()).unwrap();
    let items: Vec<starlark::values::Value<'_>> = list.iter().collect();
    assert_eq!(items.len(), 1);
    assert!(items[0].to_repr().contains("label_target.txt"));
}

#[test]
fn test_actions_declare_file_empty_filename_fails() {
    let (mut assert, _cleanup) = new_actions_assert();

    assert.pass(
        r#"
load("//rules:actions.scl", "empty_filename_rule")

empty_filename_rule(
    name = "empty_filename_target",
)
"#,
    );

    let context = assert.context();
    let label = Label::new(
        PackageRef::root().to_owned(),
        "empty_filename_target".to_owned(),
    );
    let target = context
        .session
        .get_target(label.as_ref(), context.session.default_toolchain.as_ref());

    let session = assert.session();
    let err = run(&target, move |t: &testutils::FakeTargetRef| {
        testutils::FakeEvalContext::rule_impl(session.clone(), t.clone())
    })
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("Declared file name cannot be empty"));
}

#[test]
fn test_actions_declare_file_duplicate_fails() {
    let (mut assert, _cleanup) = new_actions_assert();

    assert.pass(
        r#"
load("//rules:actions.scl", "duplicate_declare_file_rule")

duplicate_declare_file_rule(
    name = "duplicate_declare_file_target",
)
"#,
    );

    let context = assert.context();
    let label = Label::new(
        PackageRef::root().to_owned(),
        "duplicate_declare_file_target".to_owned(),
    );
    let target = context
        .session
        .get_target(label.as_ref(), context.session.default_toolchain.as_ref());

    let session = assert.session();
    let err = run(&target, move |t: &testutils::FakeTargetRef| {
        testutils::FakeEvalContext::rule_impl(session.clone(), t.clone())
    })
    .unwrap_err();

    assert!(err.to_string().contains("was already declared"));
}

#[test]
fn test_actions_already_generated_fails() {
    let (mut assert, _cleanup) = new_actions_assert();

    assert.pass(
        r#"
load("//rules:actions.scl", "already_generated_rule")

already_generated_rule(
    name = "already_generated_target",
)
"#,
    );

    let context = assert.context();
    let label = Label::new(
        PackageRef::root().to_owned(),
        "already_generated_target".to_owned(),
    );
    let target = context
        .session
        .get_target(label.as_ref(), context.session.default_toolchain.as_ref());

    let session = assert.session();
    let err = run(&target, move |t: &testutils::FakeTargetRef| {
        testutils::FakeEvalContext::rule_impl(session.clone(), t.clone())
    })
    .unwrap_err();

    assert!(err.to_string().contains("has already been generated"));
}

#[test]
fn test_actions_undeclared_output_fails() {
    let (mut assert, _cleanup) = new_actions_assert();

    assert.pass(
        r#"
load("//rules:actions.scl", "undeclared_output_rule")

undeclared_output_rule(
    name = "undeclared_output_target",
    src = "foo/file.cc",
)
"#,
    );

    let context = assert.context();
    let label = Label::new(
        PackageRef::root().to_owned(),
        "undeclared_output_target".to_owned(),
    );
    let target = context
        .session
        .get_target(label.as_ref(), context.session.default_toolchain.as_ref());

    let session = assert.session();
    let err = run(&target, move |t: &testutils::FakeTargetRef| {
        testutils::FakeEvalContext::rule_impl(session.clone(), t.clone())
    })
    .unwrap_err();

    assert!(err.to_string().contains("was not declared by this target"));
}

#[test]
fn test_actions_never_generated_fails() {
    let (mut assert, _cleanup) = new_actions_assert();

    assert.pass(
        r#"
load("//rules:actions.scl", "never_generated_rule")

never_generated_rule(
    name = "never_generated_target",
)
"#,
    );

    let context = assert.context();
    let label = Label::new(
        PackageRef::root().to_owned(),
        "never_generated_target".to_owned(),
    );
    let target = context
        .session
        .get_target(label.as_ref(), context.session.default_toolchain.as_ref());

    let session = assert.session();
    let err = run(&target, move |t: &testutils::FakeTargetRef| {
        testutils::FakeEvalContext::rule_impl(session.clone(), t.clone())
    })
    .unwrap_err();

    assert!(err.to_string().contains("was never generated"));
}
