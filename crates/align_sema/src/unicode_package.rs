//! Canonical, sealed source bridge for the explicit Unicode package.
use super::*;

fn result_string(ty: &ast::Type) -> bool {
    ast_named_type_is(ty, &["Result"], &[ast_type_string, ast_type_csv_error])
}

fn canonical(file: &ast::File) -> bool {
    if file.imports.len() != 1
        || !ast_path_is(
            &file.imports[0],
            &["pkg", "unicode", "internal", "descriptor"],
        )
    {
        return false;
    }
    let [ast::Item::Fn(version), ast::Item::Fn(nfc)] = file.items.as_slice() else {
        return false;
    };
    let version_valid = version.vis == ast::Vis::Pub
        && version.name.name == "version"
        && version.type_params.is_empty()
        && version.params.is_empty()
        && version.ret.as_ref().is_some_and(ast_type_str)
        && matches!(ast_body_single_expr(&version.body).map(|e| &e.kind),
            Some(ast::ExprKind::Str(value)) if value == "16.0.0");
    let nfc_valid = nfc.vis == ast::Vis::Pub && nfc.name.name == "nfc"
        && nfc.type_params.is_empty()
        && matches!(nfc.params.as_slice(), [input]
            if input.mode == ast::ParamMode::ByValue && input.name.name == "input" && ast_type_str(&input.ty))
        && nfc.ret.as_ref().is_some_and(result_string)
        && ast_body_single_expr(&nfc.body).is_some_and(|expression| {
            let ast::ExprKind::Call { callee, args } = &expression.kind else { return false; };
            let ast::ExprKind::FieldAccess { recv, field } = &callee.kind else { return false; };
            field.name == "nfc"
                && flatten_module_path(recv).as_deref() == Some("pkg.unicode.internal.descriptor")
                && matches!(args.as_slice(), [input]
                    if matches!(&input.kind, ast::ExprKind::Path(path) if ast_path_is(path, &["input"])))
        });
    version_valid && nfc_valid
}

pub(super) fn validate(modules: &[Module], diags: &mut Diagnostics) {
    for module in modules.iter().filter(|module| !module.interface_only) {
        let valid = match module.path.as_str() {
            "pkg.unicode" => canonical(module.file),
            "pkg.unicode.internal.descriptor" => {
                module.file.imports.is_empty() && module.file.items.is_empty()
            }
            _ => continue,
        };
        if !valid {
            let span = module
                .file
                .module
                .as_ref()
                .map_or_else(|| Span::new(0, 0, 0), |path| path.span);
            diags.error(
                format!(
                    "module `{}` must match the canonical package definition",
                    module.path
                ),
                span,
            );
        }
    }
}

impl Checker<'_, '_> {
    pub(super) fn check_unicode_nfc(&mut self, args: &[ast::Expr], span: Span) -> Expr {
        let err = Expr {
            kind: ExprKind::Bool(false),
            ty: Ty::Error,
            span,
        };
        if self.cur_module != "pkg.unicode"
            || self.cur_fn != "pkg.unicode$nfc"
            || !self
                .user_imports
                .contains("pkg.unicode.internal.descriptor")
        {
            self.diags.error("Unicode descriptor operations are compiler-private to the canonical `pkg.unicode` wrapper".to_owned(), span);
            return err;
        }
        let [input] = args else {
            self.diags
                .error("Unicode NFC expects one text argument".to_owned(), span);
            return err;
        };
        let input = self.check_str_init(input);
        if input.ty != Ty::Str {
            return err;
        }
        Expr {
            kind: ExprKind::UnicodeNfc {
                input: Box::new(input),
            },
            ty: Ty::Result(Scalar::String, Scalar::Enum(self.error_enum_id)),
            span,
        }
    }
}
