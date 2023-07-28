#![deny(clippy::all)]
use lazy_static::lazy_static;
use serde::Deserialize;
use std::collections::HashMap;
use std::vec;
use swc_core::common::sync::Lrc;
use swc_core::common::{BytePos, FilePathMapping, Span, SyntaxContext};
use swc_core::common::{FileName, SourceMap};
use swc_core::ecma::ast::{
  BinExpr, Bool, CallExpr, Callee, CondExpr, Expr, ExprOrSpread, Function, Ident, KeyValueProp,
  Lit, MemberExpr, MemberProp, ObjectLit, ParenExpr, Prop, PropName, ThisExpr, UnaryExpr,
};
use swc_core::ecma::codegen::text_writer::JsWriter;
use swc_core::ecma::codegen::Emitter;
use swc_core::ecma::parser::EsConfig;
use swc_core::ecma::parser::{Parser, StringInput, Syntax};
use swc_core::ecma::utils::ExprExt;
use swc_core::ecma::visit::{AstKindPath, Fold, FoldWith, VisitMutAstPath, VisitMutWithPath};

#[macro_use]
extern crate napi_derive;
struct BindThisVisit {
  config: Config,
  scope: HashMap<String, i32>,
  in_props: bool,
  cached_props: Vec<String>,
}

impl BindThisVisit {
  /**
   * scope {}
   * function (item) {
   *    scope { item: 1 }
   *    function(item, index) {
   *      scope { item: 2, index: 1 }
   *    }
   *    scope { item: 1 }
   * }
   * scope {}
   */
  pub fn add_scope(self: &mut BindThisVisit, key: &String) {
    let num = self.scope.get(key);
    if num.is_none() {
      self.scope.insert(key.to_string(), 1);
    } else {
      self.scope.insert(key.to_string(), num.unwrap() + 1);
    }
  }
  pub fn remove_scope(self: &mut BindThisVisit, key: &String) {
    let num = self.scope.get(key);
    if num.is_some() {
      let num = num.unwrap();
      if *num > 1 {
        self.scope.insert(key.to_string(), num - 1);
      } else {
        self.scope.remove(key);
      }
    }
  }
  pub fn has_binding(self: &mut BindThisVisit, key: &String) -> bool {
    self.scope.contains_key(key)
  }

  /**
   * this._c(args)
   */
  fn gen_bind_c_call_expr(self: &mut BindThisVisit, args: Vec<ExprOrSpread>) -> Expr {
    let span = Span {
      lo: BytePos(0),
      hi: BytePos(0),
      ctxt: SyntaxContext::empty(),
    };
    Expr::Call(CallExpr {
      span: span,
      callee: Callee::Expr(Box::new(Expr::Member(MemberExpr {
        span: span,
        obj: Box::new(Expr::This(ThisExpr { span: span })),
        prop: MemberProp::Ident(Ident {
          span: span,
          sym: "_c".into(),
          optional: false,
        }),
      }))),
      args: args,
      type_args: None,
    })
  }
  /**
   * currentState => this._c('currentState', this.currentState)
   */
  fn gen_bind_this_of_ident(self: &mut BindThisVisit, ident: &Ident) -> Option<Expr> {
    let span = ident.span;
    let name = ident.sym.to_string();
    if self.config.check_ignore(&name) || self.has_binding(&name) {
      return None;
    }
    if self.has_binding(&SCOPE_P) {
      self.cached_props.push(ident.sym.to_string());
    }
    Some(self.gen_bind_c_call_expr(vec![
      ExprOrSpread {
        spread: None,
        expr: Box::new(Expr::Lit(Lit::Str(name.clone().into()))),
      },
      ExprOrSpread {
        spread: None,
        expr: Box::new(Expr::Member(MemberExpr {
          span: span,
          obj: Box::new(Expr::This(ThisExpr { span: span })),
          prop: MemberProp::Ident(Ident {
            span: span,
            sym: name.clone().into(),
            optional: false,
          }),
        })),
      },
    ]))
  }

  /**
   * (currentState.state[0]) => this._c('contentStanderd.state[0]', this.contentStanderd.state[0])
   * (currentState.state.length) => this._c('contentStanderd.state', this.contentStanderd.state).length
   * (currentState.state[index].key) => this._c('contentStanderd.state[index].key', this.contentStanderd.state[index].key)
   * currentState.state['test'] => this._c('currentState.state.test', currentState.state['test'].key)
   *
   * scope item
   * item.state[index].key => item.state[index].key
   * let c = item
   * c.state[index].key => c.state[index].key
   * item.item
   */
  fn gen_bind_this_of_expr(self: &mut BindThisVisit, expr: &mut Expr) -> Option<Expr> {
    let mut name = String::from("");
    let mut skip = Bool::from(false);
    let res = self.depth_traverse(expr, &mut name, &mut skip);
    return res;
  }

  fn depth_traverse(
    self: &mut BindThisVisit,
    expr: &mut Expr,
    name: &mut String,
    skip: &mut Bool,
  ) -> Option<Expr> {
    if expr.is_member() {
      let member_expr = expr.as_mut_member().unwrap();
      if member_expr.obj.is_member() {
        let obj = member_expr.obj.as_mut();
        self.depth_traverse(obj, name, skip);
      }
      if !skip.value && (member_expr.obj.is_this() || member_expr.obj.is_array()) {
        skip.value = true
      }
      if !skip.value && member_expr.obj.is_ident() {
        let ident = member_expr.obj.as_ident().unwrap();
        let ident_string = ident.sym.to_string();
        if self.config.check_ignore(&ident_string) || self.has_binding(&ident_string) {
          skip.value = true
        } else {
          name.push_str(ident_string.as_str());
          let obj = Expr::Member(MemberExpr {
            span: ident.span,
            obj: Box::new(Expr::This(ThisExpr { span: ident.span })),
            prop: MemberProp::Ident(ident.clone()),
          });
          member_expr.obj = Box::new(obj);
          if self.has_binding(&SCOPE_P) {
            self.cached_props.push(ident_string);
          }
        }
      }
      if !skip.value && member_expr.prop.is_ident() {
        let ident = member_expr.prop.as_ident().unwrap();
        let ident_string = ident.sym.to_string();
        if ident_string == "length" || ident_string == "size" {
          member_expr.obj = Box::new(self.gen_bind_c_call_expr(vec![
            ExprOrSpread {
              spread: None,
              expr: Box::new(Expr::Lit(Lit::Str(name.clone().into()))),
            },
            ExprOrSpread {
              spread: None,
              expr: member_expr.obj.clone(),
            },
          ]));
          skip.value = true
        } else {
          name.push_str(".");
          name.push_str(ident_string.as_str());
        }
      }
      if member_expr.prop.is_computed() {
        let computed = member_expr.prop.as_mut_computed().unwrap();
        if computed.expr.is_lit() {
          let lit = computed.expr.as_lit().unwrap();
          match lit {
            Lit::Num(num) => {
              name.push_str("[");
              name.push_str(num.value.to_string().as_str());
              name.push_str("]");
            }
            Lit::Str(str) => {
              name.push_str(".");
              name.push_str(str.value.to_string().as_str());
            }
            _ => {}
          }
        } else {
          self.replace_expr(&mut computed.expr);
          if !skip.value {
            member_expr.obj = Box::new(self.gen_bind_c_call_expr(vec![
              ExprOrSpread {
                spread: None,
                expr: Box::new(Expr::Lit(Lit::Str(name.clone().into()))),
              },
              ExprOrSpread {
                spread: None,
                expr: member_expr.obj.clone(),
              },
            ]));
            skip.value = true;
          }
        }
      }
      if !skip.value {
        return Some(self.gen_bind_c_call_expr(vec![
          ExprOrSpread {
            spread: None,
            expr: Box::new(Expr::Lit(Lit::Str(name.clone().into()))),
          },
          ExprOrSpread {
            spread: None,
            expr: Box::new(expr.clone()),
          },
        ]));
      } else {
        return None;
      }
    } else {
      return None;
    }
  }

  fn replace_expr(self: &mut BindThisVisit, expr: &mut Expr) {
    if expr.is_ident() {
      let ident = expr.as_ident().unwrap();
      let new_expr = self.gen_bind_this_of_ident(ident);
      if new_expr.is_some() {
        std::mem::replace(expr, new_expr.unwrap());
      }
    } else if expr.is_member() {
      let new_expr = self.gen_bind_this_of_expr(expr);
      if new_expr.is_some() {
        std::mem::replace(expr, new_expr.unwrap());
      }
    }
  }

  fn replace_prop(self: &mut BindThisVisit, prop: &mut Prop) {
    if prop.is_key_value() {
      let kv = prop.as_mut_key_value().unwrap();
      self.replace_expr(&mut kv.value);
    }
    if prop.is_shorthand() {
      let shorthand = prop.as_shorthand().unwrap();
      let ident = self.gen_bind_this_of_ident(shorthand);
      if ident.is_some() {
        let new_prop = Prop::KeyValue(KeyValueProp {
          key: PropName::Ident(shorthand.clone()),
          value: Box::new(ident.unwrap()),
        });
        std::mem::replace(prop, new_prop);
      }
    }
  }
}

impl VisitMutAstPath for BindThisVisit {
  fn visit_mut_function(&mut self, node: &mut Function, path: &mut AstKindPath) {
    // 函数作用域的变量ignore掉
    (&node.params).into_iter().for_each(|v| {
      if v.pat.is_ident() {
        let ident = v.pat.as_ident().unwrap();
        self.add_scope(&ident.sym.to_string());
      }
    });
    node.visit_mut_children_with_path(self, path);
    (&node.params).into_iter().for_each(|v| {
      if v.pat.is_ident() {
        let ident = v.pat.as_ident().unwrap();
        self.remove_scope(&ident.sym.to_string());
      }
    });
  }
  fn visit_mut_cond_expr(&mut self, node: &mut CondExpr, path: &mut AstKindPath) {
    node.visit_mut_children_with_path(self, path);
    self.replace_expr(node.test.as_mut());
    self.replace_expr(node.cons.as_mut());
    self.replace_expr(node.alt.as_mut());
  }
  fn visit_mut_paren_expr(&mut self, node: &mut ParenExpr, path: &mut AstKindPath) {
    node.visit_mut_children_with_path(self, path);
    self.replace_expr(node.expr.as_mut());
  }
  fn visit_mut_expr_or_spread(&mut self, node: &mut ExprOrSpread, path: &mut AstKindPath) {
    node.visit_mut_children_with_path(self, path);
    self.replace_expr(node.expr.as_mut());
  }
  fn visit_mut_bin_expr(&mut self, node: &mut BinExpr, path: &mut AstKindPath) {
    node.visit_mut_children_with_path(self, path);
    self.replace_expr(node.left.as_mut());
    self.replace_expr(node.right.as_mut());
  }
  fn visit_mut_unary_expr(&mut self, node: &mut UnaryExpr, path: &mut AstKindPath) {
    node.visit_mut_children_with_path(self, path);
    self.replace_expr(node.arg.as_mut());
  }
  fn visit_mut_object_lit(&mut self, node: &mut ObjectLit, path: &mut AstKindPath) {
    node.visit_mut_children_with_path(self, path);
    node.props.as_mut_slice().iter_mut().for_each(|v| {
      if v.is_prop() {
        let prop = v.as_mut_prop().unwrap().as_mut();
        self.replace_prop(prop)
      }
    });
  }
  fn visit_mut_call_expr(&mut self, node: &mut CallExpr, path: &mut AstKindPath) {
    let mut is_p_call = false;
    if node.callee.is_expr() {
      let expr = node.callee.as_mut_expr().unwrap();
      if expr.is_member() {
        let member_expr = expr.as_mut_member().unwrap();
        if member_expr.prop.is_ident() {
          let ident = member_expr.prop.as_ident().unwrap();
          if ident.sym.eq("_p".into()) {
            is_p_call = true;
          }
        }
      }
      self.replace_expr(expr);
    }
    if is_p_call {
      self.add_scope(&SCOPE_P);
    }
    node.visit_mut_children_with_path(self, path);
    if is_p_call {
      self.remove_scope(&SCOPE_P);
    }
  }
}

// 移除_P方法
impl Fold for BindThisVisit {
  fn fold_expr(&mut self, node: Expr) -> Expr {
    let new_node = node.fold_children_with(self);
    if new_node.is_call() {
      let call_node = new_node.as_call().unwrap();
      if call_node.callee.is_expr() {
        let callee = call_node.callee.as_expr().unwrap();
        if callee.is_member() {
          let member_expr = callee.as_member().unwrap();
          if member_expr.obj.is_this() && member_expr.prop.is_ident() {
            let prop: &Ident = member_expr.prop.as_ident().unwrap();
            if prop.sym.eq("_p".into()) {
              let expr = call_node.args[0].expr.as_expr();
              return expr.clone();
            }
          }
        }
      }
    }
    new_node
  }
}

#[derive(Debug, Deserialize)]
#[allow(dead_code, non_snake_case)]
struct Transformed {
  code: String,
  propKeys: Vec<String>,
}

#[derive(Deserialize)]
#[allow(dead_code, non_snake_case)]
struct TestCase {
  code: String,
  file: String,
  needCollect: bool,
  ignoreMap: HashMap<String, String>,
  transformed: Transformed,
}

#[test]
fn test_transform() {
  let source_codes_content = std::fs::read("data/bind-this.json").unwrap();
  let parsed: Vec<TestCase> =
    serde_json::from_str(String::from_utf8(source_codes_content).unwrap().as_str()).unwrap();
  for mut item in parsed {
    let mut res = bind_this(
      item.code,
      Some(Config {
        ignoreMap: item.ignoreMap,
        needCollect: item.needCollect,
      }),
    );
    assert_eq!(
      res
        .code
        .replace("\n", "")
        .replace(" ", "")
        .replace("\"", "'"),
      item.transformed.code
    );
    res.props.sort();
    item.transformed.propKeys.sort();
    assert_eq!(res.props.join(","), item.transformed.propKeys.join(","))
  }
}

#[napi(object)]
pub struct Config {
  pub ignoreMap: HashMap<String, String>,
  pub needCollect: bool,
}

#[derive(Debug)]
#[napi(object)]
pub struct Res {
  pub code: String,
  pub props: Vec<String>,
}

impl Config {
  fn check_ignore(self: &Config, key: &String) -> bool {
    return self.ignoreMap.contains_key(key) || DEFAULT_IGNORE_MAP.contains_key(key);
  }
}

lazy_static! {
  /**
   * 默认的忽略配置
   */
  static ref DEFAULT_IGNORE_MAP: HashMap<String, String> = HashMap::from_iter(
    vec![
      ("Infinity", "1"),
      ("undefined", "1"),
      ("NaN", "1"),
      ("isFinite", "1"),
      ("isNaN", "1"),
      ("parseFloat", "1"),
      ("parseInt", "1"),
      ("decodeURI", "1"),
      ("decodeURIComponent", "1"),
      ("encodeURI", "1"),
      ("encodeURIComponent", "1"),
      ("Math", "1"),
      ("Number", "1"),
      ("Date", "1"),
      ("Array", "1"),
      ("Object", "1"),
      ("Boolean", "1"),
      ("String", "1"),
      ("RegExp", "1"),
      ("Map", "1"),
      ("Set", "1"),
      ("JSON", "1"),
      ("Intl", "1"),
      ("require", "1"),
      ("global", "1"),
    ]
    .iter()
    .map(|(v1, v2)| (v1.to_string(), v2.to_string()))
  );
  static ref SCOPE_P: String = String::from("_p");
}

/**
 * 格式化配置
 */
fn normalize_config(input_config: Option<Config>) -> Config {
  let config = match input_config {
    Some(input_config) => input_config,
    None => Config {
      ignoreMap: HashMap::from_iter(vec![]),
      needCollect: false,
    },
  };
  config
}

#[napi]
pub fn bind_this(source: String, input_config: Option<Config>) -> Res {
  // 合并config
  let config = normalize_config(input_config);
  // 创建一个swc parser
  let cm: Lrc<SourceMap> = Default::default();
  let fm = cm.new_source_file(FileName::Custom("".into()), source.into());
  let mut script = Parser::new(
    Syntax::Es(EsConfig::default()),
    StringInput::from(&*fm),
    None,
  )
  .parse_script()
  .unwrap();

  // visit
  let mut bind_this: BindThisVisit = BindThisVisit {
    config,
    scope: HashMap::from_iter(vec![]),
    in_props: false,
    cached_props: Vec::new(),
  };

  // transform indent
  script.visit_mut_with_path(&mut bind_this, &mut Default::default());
  script = script.fold_with(&mut bind_this);

  // 生成代码
  let pretty_code = {
    let config = swc_core::ecma::codegen::Config {
      minify: false,
      target: swc_core::ecma::ast::EsVersion::Es5,
      ascii_only: false,
      omit_last_semi: false,
    };
    let mut buf = vec![];
    let mut emitter = Emitter {
      cfg: config,
      comments: None,
      cm: Lrc::new(SourceMap::new(FilePathMapping::empty())),
      wr: Box::new(JsWriter::new(
        Lrc::new(SourceMap::default()),
        "\n",
        &mut buf,
        None,
      )),
    };
    emitter.emit_script(&script).unwrap();
    String::from_utf8_lossy(&buf).to_string()
  };

  return Res {
    code: pretty_code,
    props: bind_this.cached_props,
  };
}
