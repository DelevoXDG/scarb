use cairo_lang_defs::plugin::{MacroPluginMetadata, PluginDiagnostic, PluginGeneratedFile};
use cairo_lang_filesystem::{
    cfg::{Cfg, CfgSet},
    db::Edition,
    ids::{CodeMapping, CodeOrigin},
    span::{TextOffset, TextSpan},
};
use cairo_lang_syntax::node::db::SyntaxGroup;
use cairo_lang_utils::ordered_hash_set::OrderedHashSet;
use smol_str::SmolStr;
use std::sync::Arc;

// Same as MacroPluginMetadata but with owned values.
pub struct MetadataDataHolder {
    cfg_set: Arc<CfgSet>,
    declared_derives: OrderedHashSet<String>,
    allowed_features: OrderedHashSet<SmolStr>,
    edition: Edition,
}

impl MetadataDataHolder {
    pub fn cfg_set(&self) -> Arc<CfgSet> {
        self.cfg_set.clone()
    }

    pub fn new(metadata: proc_macro_server_api::MacroPluginMetadata) -> Self {
        Self {
            cfg_set: CfgSet::from_iter(metadata.cfg_set.cfgs.into_iter().map(|cfg| Cfg {
                key: cfg.key.into(),
                value: cfg.value.map(Into::into),
            }))
            .into(),
            allowed_features: OrderedHashSet::from_iter(
                metadata.allowed_features.into_iter().map(Into::into),
            ),
            declared_derives: OrderedHashSet::from_iter(
                metadata.declared_derives.into_iter().map(Into::into),
            ),
            edition: match proc_macro_server_api::Edition::try_from(metadata.edition).unwrap() {
                proc_macro_server_api::Edition::V202301 => Edition::V2023_01,
                proc_macro_server_api::Edition::V202310 => Edition::V2023_10,
                proc_macro_server_api::Edition::V202311 => Edition::V2023_11,
                proc_macro_server_api::Edition::V202407 => Edition::V2024_07,
            },
        }
    }

    pub fn macro_plugin_metadata(&self) -> MacroPluginMetadata<'_> {
        MacroPluginMetadata {
            cfg_set: &self.cfg_set,
            allowed_features: &self.allowed_features,
            declared_derives: &self.declared_derives,
            edition: self.edition,
        }
    }
}

pub fn plugin_generated_file(
    value: PluginGeneratedFile,
) -> proc_macro_server_api::PluginGeneratedFile {
    proc_macro_server_api::PluginGeneratedFile {
        code_mappings: value.code_mappings.into_iter().map(code_mapping).collect(),
        content: value.content,
        name: value.name.into(),
    }
}

pub fn plugin_diagnostic(
    value: PluginDiagnostic,
    db: &dyn SyntaxGroup,
) -> proc_macro_server_api::PluginDiagnostic {
    proc_macro_server_api::PluginDiagnostic {
        message: value.message,
        severity: value.severity as i32,
        stable_ptr: to_u32(value.stable_ptr.lookup(db).offset()),
    }
}

fn code_mapping(value: CodeMapping) -> proc_macro_server_api::CodeMapping {
    proc_macro_server_api::CodeMapping {
        span: text_span(value.span),
        origin: code_origin(value.origin),
    }
}

fn to_u32(text_offset: TextOffset) -> u32 {
    unsafe { std::mem::transmute(text_offset) } //TODO
}

fn text_span(value: TextSpan) -> proc_macro_server_api::TextSpan {
    proc_macro_server_api::TextSpan {
        start: to_u32(value.start),
        end: to_u32(value.end),
    }
}

fn code_origin(value: CodeOrigin) -> proc_macro_server_api::CodeOrigin {
    proc_macro_server_api::CodeOrigin {
        origin: Some(match value {
            CodeOrigin::Start(start) => {
                proc_macro_server_api::code_origin::Origin::Start(to_u32(start))
            }
            CodeOrigin::Span(span) => {
                proc_macro_server_api::code_origin::Origin::Span(text_span(span))
            }
        }),
    }
}
