use std::{
    mem::replace,
    sync::atomic::{AtomicBool, Ordering},
};

use once_cell::sync::Lazy;
use rustc_hash::FxHashMap;
use swc_atoms::JsWord;
use swc_common::{FileName, FilePathMapping, Mark, SourceMap, SyntaxContext, DUMMY_SP};
use swc_ecma_ast::*;
use swc_ecma_utils::{prepend_stmts, quote_ident, DropSpan, ExprFactory};
use swc_ecma_visit::{as_folder, noop_visit_mut_type, Fold, VisitMut, VisitMutWith};

#[macro_export]
macro_rules! enable_helper {
    ($i:ident) => {{
        $crate::helpers::HELPERS.with(|helpers| {
            helpers.$i();
            helpers.mark()
        })
    }};
}

fn parse(code: &str) -> Vec<Stmt> {
    let cm = SourceMap::new(FilePathMapping::empty());

    let fm = cm.new_source_file(FileName::Custom(stringify!($name).into()), code.into());
    swc_ecma_parser::parse_file_as_script(
        &fm,
        Default::default(),
        Default::default(),
        None,
        &mut vec![],
    )
    .map(|mut script| {
        script.body.visit_mut_with(&mut DropSpan {
            preserve_ctxt: false,
        });
        script.body
    })
    .map_err(|e| {
        unreachable!("Error occurred while parsing error: {:?}", e);
    })
    .unwrap()
}

macro_rules! add_to {
    ($buf:expr, $name:ident, $b:expr, $mark:expr) => {{
        static STMTS: Lazy<Vec<Stmt>> = Lazy::new(|| {
            let code = include_str!(concat!("./_", stringify!($name), ".js"));
            parse(&code)
        });

        let enable = $b.load(Ordering::Relaxed);
        if enable {
            $buf.extend(STMTS.iter().cloned().map(|mut stmt| {
                stmt.visit_mut_with(&mut Marker {
                    base: SyntaxContext::empty().apply_mark($mark),
                    decls: Default::default(),

                    decl_ctxt: SyntaxContext::empty().apply_mark(Mark::new()),
                });
                stmt
            }))
        }
    }};
}

macro_rules! add_import_to {
    ($buf:expr, $name:ident, $b:expr, $mark:expr) => {{
        let enable = $b.load(Ordering::Relaxed);
        if enable {
            let s = ImportSpecifier::Named(ImportNamedSpecifier {
                span: DUMMY_SP,
                local: Ident::new(
                    concat!("_", stringify!($name)).into(),
                    DUMMY_SP.apply_mark($mark),
                ),
                imported: Some(quote_ident!("_").into()),
                is_type_only: false,
            });

            let src: Str = concat!("@swc/helpers/_/_", stringify!($name)).into();

            $buf.push(ModuleItem::ModuleDecl(ModuleDecl::Import(ImportDecl {
                span: DUMMY_SP,
                specifiers: vec![s],
                src: Box::new(src),
                with: Default::default(),
                type_only: Default::default(),
            })))
        }
    }};
}

better_scoped_tls::scoped_tls!(
    /// This variable is used to manage helper scripts like `_inherits` from babel.
    ///
    /// The instance contains flags where each flag denotes if a helper script should be injected.
    pub static HELPERS: Helpers
);

/// Tracks used helper methods. (e.g. __extends)
#[derive(Debug, Default)]
pub struct Helpers {
    external: bool,
    mark: HelperMark,
    inner: Inner,
}

impl Helpers {
    pub fn new(external: bool) -> Self {
        Helpers {
            external,
            mark: Default::default(),
            inner: Default::default(),
        }
    }

    pub const fn mark(&self) -> Mark {
        self.mark.0
    }

    pub const fn external(&self) -> bool {
        self.external
    }
}

#[derive(Debug, Clone, Copy)]
struct HelperMark(Mark);
impl Default for HelperMark {
    fn default() -> Self {
        HelperMark(Mark::new())
    }
}

macro_rules! define_helpers {
    (
        Helpers {
            $( $name:ident : ( $( $dep:ident ),* ), )*
        }
    ) => {
        #[derive(Debug,Default)]
        struct Inner {
            $( $name: AtomicBool, )*
        }

        impl Helpers {
            $(
                pub fn $name(&self) {
                    self.inner.$name.store(true, Ordering::Relaxed);

                    if !self.external {
                        $(
                            self.$dep();
                        )*
                    }
                }
            )*
        }

        impl Helpers {
            pub fn extend_from(&self, other: &Self) {
                $(
                    if other.inner.$name.load(Ordering::SeqCst) {
                        self.inner.$name.store(true, Ordering::Relaxed);
                    }
                )*
            }
        }

        impl InjectHelpers {
            fn is_helper_used(&self) -> bool{

                HELPERS.with(|helpers|{
                    false $(
                      || helpers.inner.$name.load(Ordering::Relaxed)
                    )*
                })
            }

            fn build_helpers(&self) -> Vec<Stmt> {
                let mut buf = vec![];

                HELPERS.with(|helpers|{
                    debug_assert!(!helpers.external);
                    $(
                            add_to!(buf, $name, helpers.inner.$name, helpers.mark.0);
                    )*
                });

                buf
            }

            fn build_imports(&self) -> Vec<ModuleItem> {
                let mut buf = vec![];

                HELPERS.with(|helpers|{
                    debug_assert!(helpers.external);
                    $(
                            add_import_to!(buf, $name, helpers.inner.$name, helpers.mark.0);
                    )*
                });

                buf
            }

            fn build_requires(&self) -> Vec<Stmt>{
                let mut buf = vec![];
                HELPERS.with(|helpers|{
                    debug_assert!(helpers.external);
                    $(
                        let enable = helpers.inner.$name.load(Ordering::Relaxed);
                        if enable {
                            buf.push(self.build_reqire(stringify!($name), helpers.mark.0))
                        }
                        // add_require_to!(buf, $name, helpers.inner.$name, helpers.mark.0, self.global_mark);
                    )*
                });
                buf
            }
        }
    };
}

define_helpers!(Helpers {
    apply_decorated_descriptor: (),
    array_like_to_array: (),
    array_with_holes: (),
    array_without_holes: (array_like_to_array),
    assert_this_initialized: (),
    async_generator: (await_value),
    async_generator_delegate: (),
    async_iterator: (),
    async_to_generator: (),
    await_async_generator: (await_value),
    await_value: (),
    check_private_redeclaration: (),
    class_apply_descriptor_destructure: (),
    class_apply_descriptor_get: (),
    class_apply_descriptor_set: (),
    class_apply_descriptor_update: (),
    class_call_check: (),
    class_check_private_static_field_descriptor: (),
    class_extract_field_descriptor: (),
    class_name_tdz_error: (),
    class_private_field_get: (class_extract_field_descriptor, class_apply_descriptor_get),
    class_private_field_init: (check_private_redeclaration),
    class_private_field_loose_base: (),
    class_private_field_loose_key: (),
    class_private_field_set: (class_extract_field_descriptor, class_apply_descriptor_set),
    class_private_field_update: (
        class_extract_field_descriptor,
        class_apply_descriptor_update
    ),
    class_private_method_get: (),
    class_private_method_init: (check_private_redeclaration),
    class_private_method_set: (),
    class_static_private_field_spec_get: (
        class_check_private_static_access,
        class_check_private_static_field_descriptor,
        class_apply_descriptor_get
    ),
    class_static_private_field_spec_set: (
        class_check_private_static_access,
        class_check_private_static_field_descriptor,
        class_apply_descriptor_set
    ),
    class_static_private_field_update: (
        class_check_private_static_access,
        class_check_private_static_field_descriptor,
        class_apply_descriptor_update
    ),
    construct: (is_native_reflect_construct, set_prototype_of),
    create_class: (),
    decorate: (to_array, to_property_key),
    defaults: (),
    define_enumerable_properties: (),
    define_property: (),
    export_star: (),
    extends: (),
    get: (super_prop_base),
    get_prototype_of: (),
    inherits: (set_prototype_of),
    inherits_loose: (),
    initializer_define_property: (),
    initializer_warning_helper: (),
    instanceof: (),
    interop_require_default: (),
    interop_require_wildcard: (),
    is_native_function: (),
    iterable_to_array: (),
    iterable_to_array_limit: (),
    iterable_to_array_limit_loose: (),
    jsx: (),
    new_arrow_check: (),
    non_iterable_rest: (),
    non_iterable_spread: (),
    object_destructuring_empty: (),
    object_spread: (define_property),
    object_spread_props: (),
    object_without_properties: (object_without_properties_loose),
    object_without_properties_loose: (),
    possible_constructor_return: (type_of, assert_this_initialized),
    read_only_error: (),
    set: (super_prop_base, define_property),
    set_prototype_of: (),
    skip_first_generator_next: (),
    sliced_to_array: (
        array_with_holes,
        iterable_to_array_limit,
        unsupported_iterable_to_array,
        non_iterable_rest
    ),
    sliced_to_array_loose: (
        array_with_holes,
        iterable_to_array_limit_loose,
        unsupported_iterable_to_array,
        non_iterable_rest
    ),
    super_prop_base: (get_prototype_of),
    tagged_template_literal: (),
    tagged_template_literal_loose: (),
    // temporal_ref: (temporal_undefined),
    // temporal_undefined: (),
    throw: (),
    to_array: (
        array_with_holes,
        iterable_to_array,
        unsupported_iterable_to_array,
        non_iterable_rest
    ),
    to_consumable_array: (
        array_without_holes,
        iterable_to_array,
        unsupported_iterable_to_array,
        non_iterable_spread
    ),
    to_primitive: (type_of),
    to_property_key: (type_of, to_primitive),
    update: (get, set),
    type_of: (),
    unsupported_iterable_to_array: (array_like_to_array),
    wrap_async_generator: (async_generator),
    wrap_native_super: (
        construct,
        get_prototype_of,
        set_prototype_of,
        is_native_function
    ),
    write_only_error: (),

    class_private_field_destructure: (
        class_extract_field_descriptor,
        class_apply_descriptor_destructure
    ),
    class_static_private_field_destructure: (
        class_check_private_static_access,
        class_extract_field_descriptor,
        class_apply_descriptor_destructure
    ),

    class_static_private_method_get: (class_check_private_static_access),
    class_check_private_static_access: (),

    is_native_reflect_construct: (),

    create_super: (
        get_prototype_of,
        is_native_reflect_construct,
        possible_constructor_return
    ),

    create_for_of_iterator_helper_loose: (unsupported_iterable_to_array),

    ts_decorate: (),
    ts_generator: (),
    ts_metadata: (),
    ts_param: (),
    ts_values: (),

    apply_decs_2203_r: (),
    identity: (),
    dispose: (),
    using: (),
});

pub fn inject_helpers(global_mark: Mark) -> impl Fold + VisitMut {
    as_folder(InjectHelpers {
        global_mark,
        helper_ctxt: None,
    })
}

struct InjectHelpers {
    global_mark: Mark,
    helper_ctxt: Option<SyntaxContext>,
}

impl InjectHelpers {
    fn make_helpers_for_module(&mut self) -> Vec<ModuleItem> {
        let (helper_mark, external) = HELPERS.with(|helper| (helper.mark(), helper.external()));
        if external {
            if self.is_helper_used() {
                self.helper_ctxt = Some(SyntaxContext::empty().apply_mark(helper_mark));
                self.build_imports()
            } else {
                vec![]
            }
        } else {
            self.build_helpers()
                .into_iter()
                .map(ModuleItem::Stmt)
                .collect()
        }
    }

    fn make_helpers_for_script(&mut self) -> Vec<Stmt> {
        let (helper_mark, external) = HELPERS.with(|helper| (helper.mark(), helper.external()));

        if external {
            if self.is_helper_used() {
                self.helper_ctxt = Some(SyntaxContext::empty().apply_mark(helper_mark));
                self.build_requires()
            } else {
                Default::default()
            }
        } else {
            self.build_helpers()
        }
    }

    fn build_reqire(&self, name: &str, mark: Mark) -> Stmt {
        let c = CallExpr {
            span: DUMMY_SP,
            callee: Expr::Ident(Ident {
                span: DUMMY_SP.apply_mark(self.global_mark),
                sym: "require".into(),
                optional: false,
            })
            .as_callee(),
            args: vec![Str {
                span: DUMMY_SP,
                value: format!("@swc/helpers/_/_{}", name).into(),
                raw: None,
            }
            .as_arg()],
            type_args: None,
        };
        let decl = Decl::Var(
            VarDecl {
                span: DUMMY_SP,
                kind: VarDeclKind::Var,
                declare: false,
                decls: vec![VarDeclarator {
                    span: DUMMY_SP,
                    name: Pat::Ident(
                        Ident::new(format!("_{}", name).into(), DUMMY_SP.apply_mark(mark)).into(),
                    ),
                    init: Some(c.into()),
                    definite: false,
                }],
            }
            .into(),
        );
        Stmt::Decl(decl)
    }

    fn map_helper_ref_ident(&mut self, ref_ident: &Ident) -> Option<Expr> {
        self.helper_ctxt
            .filter(|ctxt| ctxt == &ref_ident.span.ctxt)
            .map(|_| {
                let ident = ref_ident.clone().without_loc();

                MemberExpr {
                    span: ref_ident.span,
                    obj: Box::new(ident.into()),
                    prop: quote_ident!("_").into(),
                }
                .into()
            })
    }
}

impl VisitMut for InjectHelpers {
    noop_visit_mut_type!();

    fn visit_mut_module(&mut self, module: &mut Module) {
        let helpers = self.make_helpers_for_module();

        prepend_stmts(&mut module.body, helpers.into_iter());
    }

    fn visit_mut_script(&mut self, script: &mut Script) {
        let helpers = self.make_helpers_for_script();
        let helpers_is_empty = helpers.is_empty();

        prepend_stmts(&mut script.body, helpers.into_iter());

        if !helpers_is_empty {
            script.visit_mut_children_with(self);
        }
    }

    fn visit_mut_expr(&mut self, n: &mut Expr) {
        match n {
            Expr::Ident(ref_ident) => {
                if let Some(expr) = self.map_helper_ref_ident(ref_ident) {
                    *n = expr;
                }
            }

            _ => n.visit_mut_children_with(self),
        };
    }
}

struct Marker {
    base: SyntaxContext,
    decls: FxHashMap<JsWord, SyntaxContext>,

    decl_ctxt: SyntaxContext,
}

impl VisitMut for Marker {
    noop_visit_mut_type!();

    fn visit_mut_fn_decl(&mut self, n: &mut FnDecl) {
        let old_decl_ctxt = replace(
            &mut self.decl_ctxt,
            SyntaxContext::empty().apply_mark(Mark::new()),
        );
        let old_decls = self.decls.clone();

        n.visit_mut_children_with(self);

        self.decls = old_decls;
        self.decl_ctxt = old_decl_ctxt;
    }

    fn visit_mut_fn_expr(&mut self, n: &mut FnExpr) {
        let old_decl_ctxt = replace(
            &mut self.decl_ctxt,
            SyntaxContext::empty().apply_mark(Mark::new()),
        );
        let old_decls = self.decls.clone();

        n.visit_mut_children_with(self);

        self.decls = old_decls;
        self.decl_ctxt = old_decl_ctxt;
    }

    fn visit_mut_ident(&mut self, i: &mut Ident) {
        i.span.ctxt = self.decls.get(&i.sym).copied().unwrap_or(self.base);
    }

    fn visit_mut_member_prop(&mut self, p: &mut MemberProp) {
        if let MemberProp::Computed(p) = p {
            p.visit_mut_with(self);
        }
    }

    fn visit_mut_param(&mut self, n: &mut Param) {
        if let Pat::Ident(i) = &n.pat {
            self.decls.insert(i.id.sym.clone(), self.decl_ctxt);
        }

        n.visit_mut_children_with(self);
    }

    fn visit_mut_prop_name(&mut self, n: &mut PropName) {
        if let PropName::Computed(e) = n {
            e.visit_mut_with(self);
        }
    }

    fn visit_mut_super_prop(&mut self, p: &mut SuperProp) {
        if let SuperProp::Computed(p) = p {
            p.visit_mut_with(self);
        }
    }

    fn visit_mut_var_declarator(&mut self, v: &mut VarDeclarator) {
        if let Pat::Ident(i) = &mut v.name {
            if &*i.id.sym == "id" {
                i.id.span.ctxt = self.base;
                self.decls.insert(i.id.sym.clone(), self.base);
                return;
            }

            if !i.id.sym.starts_with("__") {
                self.decls.insert(i.id.sym.clone(), self.decl_ctxt);
            }
        }

        v.visit_mut_children_with(self);
    }
}

#[cfg(test)]
mod tests {
    use swc_ecma_visit::{as_folder, FoldWith};
    use testing::DebugUsingDisplay;

    use super::*;
    use crate::pass::noop;

    #[test]
    fn external_helper() {
        let input = "_throw()";
        crate::tests::Tester::run(|tester| {
            HELPERS.set(&Helpers::new(true), || {
                let expected = tester.apply_transform(
                    as_folder(DropSpan {
                        preserve_ctxt: false,
                    }),
                    "output.js",
                    Default::default(),
                    "import { _ as _throw } from \"@swc/helpers/_/_throw\";
_throw();",
                )?;
                enable_helper!(throw);

                eprintln!("----- Actual -----");

                let tr = as_folder(inject_helpers(Mark::new()));
                let actual = tester
                    .apply_transform(tr, "input.js", Default::default(), input)?
                    .fold_with(&mut crate::hygiene::hygiene())
                    .fold_with(&mut crate::fixer::fixer(None));

                if actual == expected {
                    return Ok(());
                }

                let (actual_src, expected_src) = (tester.print(&actual), tester.print(&expected));

                if actual_src == expected_src {
                    return Ok(());
                }

                println!(">>>>> Orig <<<<<\n{}", input);
                println!(">>>>> Code <<<<<\n{}", actual_src);
                assert_eq!(
                    DebugUsingDisplay(&actual_src),
                    DebugUsingDisplay(&expected_src)
                );
                Err(())
            })
        });
    }

    #[test]
    fn use_strict_before_helper() {
        crate::tests::test_transform(
            Default::default(),
            |_| {
                enable_helper!(throw);
                as_folder(inject_helpers(Mark::new()))
            },
            "'use strict'",
            "'use strict'
function _throw(e) {
    throw e;
}
",
            false,
            Default::default,
        )
    }

    #[test]
    fn name_conflict() {
        crate::tests::test_transform(
            Default::default(),
            |_| {
                enable_helper!(throw);
                as_folder(inject_helpers(Mark::new()))
            },
            "let _throw = null",
            "function _throw(e) {
    throw e;
}
let _throw1 = null;
",
            false,
            Default::default,
        )
    }
    #[test]
    fn use_strict_abort() {
        crate::tests::test_transform(
            Default::default(),
            |_| noop(),
            "'use strict'

let x = 4;",
            "'use strict'

let x = 4;",
            false,
            Default::default,
        );
    }
}
