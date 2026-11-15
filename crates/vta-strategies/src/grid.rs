//! 声明式参数网格: 字段声明、笛卡尔展开、范围校验与参数编码同源。
//!
//! C3 让策略的 `params` 文本、JSON 键值与 `strategy_params` 整数由同一处产出,
//! 但每个策略的**维度声明**仍散落在三处:
//!
//! 1. `config.rs` 的结构体字段与 serde 默认值;
//! 2. `pairs()` / `triples()` / `quints()` 的嵌套循环与展开顺序;
//! 3. `validate()` 的范围, 以及 `strategy.rs` 里 `param_set()` 的键名与顺序。
//!
//! 三处只要漏掉一维, 就会出现"配置能解析、网格少一维、编码撞车"的静默错误。
//! [`grid_config!`](crate::grid::grid_config) 把这三处收敛到一次声明: 宏生成
//! 结构体、展开方法、范围校验与 `ParamSet` 构造; 策略侧只保留一行
//! `<Config>::param_set(&self.pairs[idx])` 的委托。
//!
//! 非笛卡尔网格或带派生变换的特例(`macd` 的显式三元组、`clenow` 的日期/
//! 模式过滤、`sgma` 的可选维度、`mabb_*` 的 `stopk==0` 去重等)保留手写
//! 入口 —— 强行套宏只会把特例藏进宏参数里。

use crate::config::{F64Param, PeriodParam};

/// 一个可展开的网格维度。
///
/// 只为参数类型实现; 展开顺序与值类型必须与对应配置字段一致。
pub(crate) trait GridDim {
    /// 展开后的取值类型。
    type Value;

    /// 展开为取值列表(顺序即笛卡尔积顺序)。
    fn grid_values(&self) -> Vec<Self::Value>;
}

impl GridDim for F64Param {
    type Value = f64;

    fn grid_values(&self) -> Vec<Self::Value> {
        self.values()
    }
}

impl GridDim for PeriodParam {
    type Value = usize;

    fn grid_values(&self) -> Vec<Self::Value> {
        self.values()
    }
}

impl GridDim for Vec<String> {
    type Value = String;

    fn grid_values(&self) -> Vec<Self::Value> {
        self.clone()
    }
}

/// 递归生成嵌套循环; 每层把当前取值追加到 `acc`, 最内层写入 `out`。
///
/// 不能在一个 `macro_rules!` 重复里拆开 `for {` 与 `}`(定界符必须成对),
/// 故用独立递归宏生成**平衡**的嵌套块。
macro_rules! grid_push_loops {
    ($out:ident, [$field:ident $(, $rest:ident)*], [$($acc:ident),*]) => {
        for $field in &$field {
            $crate::grid::grid_push_loops!($out, [$($rest),*], [$($acc,)* $field]);
        }
    };
    ($out:ident, [], [$($acc:ident),*]) => {
        $out.push(($((*$acc).clone()),*));
    };
}

pub(crate) use grid_push_loops;

/// 对展开结果应用声明式过滤; 单独成宏以避免与外层字段重复组嵌套。
///
/// `filter true;` 直接展开为空, 不产生额外的 `retain` 遍历; 其余过滤写成
/// `filter (expr);`, 括号让它成为单个 token tree, 便于与 `true` 区分。
macro_rules! grid_retain {
    ($out:ident, [$($field:ident),+], true) => {};
    ($out:ident, [$($field:ident),+], ($expr:expr)) => {
        $out.retain(|($($field),+)| {
            $(let _ = &$field;)+
            $expr
        });
    };
}

pub(crate) use grid_retain;

/// 把元组条目解构为字段绑定; 单字段条目不是元组, 单独处理。
macro_rules! grid_bind {
    ($item:expr, [$field:ident]) => {
        let $field = $item.clone();
    };
    ($item:expr, [$($field:ident),+]) => {
        let ($($field),+) = $item.clone();
    };
}

pub(crate) use grid_bind;

/// 由一张字段表生成配置结构体、笛卡尔展开、校验与参数构造。
///
/// 语法(固定顺序, 便于 `macro_rules!` 解析):
///
/// ```ignore
/// grid_config! {
///     /// 结构体文档。
///     pub struct BBRKConfig;
///     fields {
///         /// 字段文档。
///         sigma: F64Param as f64, key "sigma", label "bbrk.sigma", range (0.1, 10.0);
///         /// 字段文档。
///         bblen: PeriodParam as usize, key "bblen", label "bbrk.bblen",
///             range (2, 2000), default "crate::config::default_bblen";
///     }
///     expand pairs;
///     code |_text| code::sigma_pair(sigma, bblen as f64);
///     filter true;
///     empty "bbrk 参数组合为空";
/// }
/// ```
///
/// 字段行按出现顺序决定笛卡尔积顺序、`params` 文本键序与 JSON 键序:
///
/// - `key`: `params` / JSON 的键名; `label`: 报错时的参数名。
/// - `range (lo, hi)`: 调用 `validate_range`(展开非空且落在闭区间内)。
/// - `validate min`: 调用 `validate_named`(下界、`start <= end`、`step > 0`)。
/// - `default "path"`: 生成 `#[serde(default = "path")]`。
/// - `names EXPR`: 字段是均线名单, 逐项检查是否在 `EXPR` 中。
///
/// `expand` 是展开方法名(`pairs` / `triples` / `quints`; 单字段配置惯例用
/// `values`, 返回 `Vec<T>` 而非一元组)。
///
/// `code` 闭包里的 `text` 是最终 `params` 文本(与 `ParamSetBuilder` 拼出的完全
/// 一致), 需要 FNV 回退的历史族用它计算编码; 其余族写成 `|_text|`。
///
/// `filter true;` 表示不过滤; 需要过滤时写 `filter (expr);`, 括号把表达式收成
/// 单个 token tree, 宏才能与 `true` 区分。过滤发生在完整笛卡尔积之后, 顺序与
/// 手写 `pairs()` 的 `continue` 一致。
macro_rules! grid_config {
    (
        $(#[$struct_doc:meta])*
        pub struct $config:ident;
        fields {
            $(
                $(#[$field_doc:meta])*
                $field:ident: $param:ty as $value:ty
                    , key $key:literal
                    , label $label:literal
                    $(, range ($lo:expr, $hi:expr))?
                    $(, validate $min:literal)?
                    $(, default $default:literal)?
                    $(, names $names:expr)?
                ;
            )+
        }
        expand $expand:ident;
        code |$text:ident| $code:expr;
        $(text $text_expr:expr;)?
        filter $filter_expr:tt;
        empty $empty:literal;
    ) => {
        $(#[$struct_doc])*
        #[derive(Debug, Clone, ::serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        pub struct $config {
            $(
                $(#[$field_doc])*
                $(#[serde(default = $default)])?
                pub $field: $param,
            )+
        }

        /// # 单字段展开
        ///
        /// 单字段配置的返回值是裸值(`Vec<f64>`、`&f64`), 不是一元组; 宏里
        /// 统一的 `($value)` 写法在该情形会触发 `unused_parens`, 故整体放行。
        #[allow(unused_parens)]
        impl $config {
            /// 展开后的参数组合(声明顺序的笛卡尔积)。
            #[must_use]
            pub fn $expand(&self) -> Vec<($($value),+)> {
                $(
                    let $field: Vec<$value> = $crate::grid::GridDim::grid_values(&self.$field);
                )+
                let cap = $crate::grid_prealloc(&[$($field.len()),+]);
                let mut out = Vec::with_capacity(cap);
                $crate::grid::grid_push_loops!(out, [$($field),+], []);
                $crate::grid::grid_retain!(out, [$($field),+], $filter_expr);
                out
            }

            /// 语义校验: 范围、名单与网格非空性。
            ///
            /// # Errors
            /// 任一参数越界、名单含未知项或展开为空时返回
            /// [`AppError`](vta_core::error::AppError)。
            pub fn validate(&self) -> vta_core::error::Result<()> {
                $(
                    $(self.$field.validate_range($label, $lo, $hi)?;)?
                    $(self.$field.validate_named($label, $min)?;)?
                )+
                $(
                    $(
                        for name in &self.$field {
                            if !$names.contains(&name.as_str()) {
                                return Err(vta_core::error::AppError::Msg(format!(
                                    "{} 未知均线 {name:?}, 可选 {}",
                                    $empty.trim_end_matches(" 参数组合为空"),
                                    $names.join(", ")
                                )));
                            }
                        }
                    )?
                )+
                if self.$expand().is_empty() {
                    return Err(vta_core::error::AppError::Msg($empty.into()));
                }
                Ok(())
            }

            /// 第 `item` 组参数的结构化表示(文本 / JSON / 整数编码同源)。
            #[must_use]
            pub fn param_set(item: &($($value),+)) -> $crate::ParamSet {
                $crate::grid::grid_bind!(item, [$($field),+]);
                let builder = $crate::ParamSet::builder(0)
                    $(.kv($key, &$field))+;
                $(let builder = builder.text($text_expr);)?
                let (text, pairs) = builder.into_parts();
                let code = (|$text: &str| -> u64 { $code })(text.as_str());
                $crate::ParamSet::from_parts(text, pairs, code)
            }
        }
    };
}

pub(crate) use grid_config;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::{F64Param, PeriodParam};
    use crate::config::RangeParam;

    grid_config! {
        /// 测试用配置。
        pub struct DemoConfig;
        fields {
            /// 布林带倍数。
            sigma: F64Param as f64, key "sigma", label "demo.sigma", range (0.1, 10.0);
            /// 周期。
            bblen: PeriodParam as usize, key "bblen", label "demo.bblen",
                range (2, 2000), default "crate::config::default_bblen";
            /// 量能窗口。
            voln: PeriodParam as usize, key "voln", label "demo.voln", range (2, 500);
        }
        expand pairs;
        code |_text| crate::params::code::packed(&[
            (crate::params::code::scaled(sigma, 1000.0), 14),
            (bblen as u64, 12),
            (voln as u64, 9),
        ]);
        filter true;
        empty "demo 参数组合为空";
    }

    #[test]
    fn expands_in_declaration_order_and_honours_default() {
        let c: DemoConfig = toml::from_str("sigma = 2.5\nvoln = 10").unwrap();
        assert_eq!(c.pairs(), vec![(2.5, 20, 10)]);
    }

    #[test]
    fn cartesian_order_is_field_order() {
        let c = DemoConfig {
            sigma: F64Param::Range(RangeParam {
                start: 1.5,
                end: 2.0,
                step: 0.5,
            }),
            bblen: PeriodParam::Single(20),
            voln: PeriodParam::Range(RangeParam {
                start: 5,
                end: 10,
                step: 5,
            }),
        };
        assert_eq!(
            c.pairs(),
            vec![(1.5, 20, 5), (1.5, 20, 10), (2.0, 20, 5), (2.0, 20, 10)]
        );
    }

    #[test]
    fn param_set_matches_legacy_builder_text_and_code() {
        let p = DemoConfig::param_set(&(2.5, 20, 10));
        assert_eq!(p.text(), "sigma=2.5,bblen=20,voln=10");
        assert_eq!(p.json(), r#"{"sigma":"2.5","bblen":"20","voln":"10"}"#);
        assert_eq!(
            p.code(),
            crate::params::code::packed(&[(2500, 14), (20, 12), (10, 9)])
        );
    }

    #[test]
    fn validation_rejects_out_of_range_and_empty_grid() {
        let mut c: DemoConfig = toml::from_str("sigma = 20\nvoln = 10").unwrap();
        assert!(c.validate().is_err());

        c.sigma = F64Param::Single(2.5);
        c.voln = PeriodParam::Range(RangeParam {
            start: 5,
            end: 1,
            step: 1,
        });
        let err = c.validate().expect_err("空网格必须被拒");
        let msg = format!("{err}");
        assert!(msg.contains("展开为空") || msg.contains("demo 参数组合为空"));
    }
}
