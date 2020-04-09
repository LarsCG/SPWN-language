//! Defining all native types (and functions?)

use crate::compiler_types::*;
use crate::levelstring::*;
//use std::collections::HashMap;

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Group {
    pub id: u16,
}

#[derive(Debug, Copy, Clone, PartialEq)]

pub struct Color {
    pub id: u16,
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Block {
    pub id: u16,
}

impl Block {}

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Item {
    pub id: u16,
}

pub fn context_trigger(context: Context, globals: &mut Globals) -> GDObj {
    GDObj {
        obj_id: 0,
        groups: vec![context.start_group],
        target: Group { id: 0 },
        spawn_triggered: context.spawn_triggered,
        params: Vec::new(),
        x: context.x,
        y: 50
            - match globals.lowest_y.get_mut(&context.x) {
                Some(max) => {
                    if context.y > *max {
                        (*max) = context.y;
                        context.y
                    } else {
                        (*max) += 1;
                        *max
                    }
                }
                None => {
                    (*globals).lowest_y.insert(context.x, context.y);
                    context.y
                }
            },
    }
}

const TYPE_MEMBER_NAME: &str = "TYPE";
impl Value {
    pub fn member(
        &self,
        member: String,
        context: &Context,
        globals: &mut Globals,
        _: CompilerInfo,
    ) -> Option<Value> {
        //println!("{:?}", context.implementations);
        let get_impl = |t: String, m: String| match context.implementations.get(&(t)) {
            Some(imp) => match imp.get(&m) {
                Some(mem) => Some((*globals).stored_values[*mem as usize].clone()),
                None => None,
            },
            None => None,
        };
        let my_type = match self {
            Value::Dict(dict) => match dict.get(TYPE_MEMBER_NAME) {
                Some(value) => match (*globals).stored_values[*value as usize].clone() {
                    Value::Str(s) => s,
                    _ => unreachable!(),
                },
                None => "dictionary".to_string(),
            },

            Value::Func(_) => "function".to_string(),
            Value::Group(_) => "group".to_string(),
            Value::Color(_) => "color".to_string(),
            Value::Block(_) => "block".to_string(),
            Value::Item(_) => "item".to_string(),
            Value::Number(_) => "number".to_string(),
            Value::Bool(_) => "boolean".to_string(),
            Value::Macro(_) => "macro".to_string(),
            Value::Str(_) => "string".to_string(),
            Value::Array(_) => "array".to_string(),
            Value::Obj(_) => "object".to_string(),
            Value::Null => "null".to_string(),
        };
        if member == TYPE_MEMBER_NAME {
            return Some(Value::Str(my_type.to_string()));
        } else {
            match self {
                Value::Dict(dict) => match dict.get(&member) {
                    Some(value) => Some((*globals).stored_values[*value as usize].clone()),
                    None => get_impl(my_type.to_string(), member).clone(),
                },
                Value::Func(f) => {
                    if &member == "start_group" {
                        Some(Value::Group(f.start_group))
                    } else {
                        get_impl(my_type.to_string(), member).clone()
                    }
                }
                _ => get_impl(my_type.to_string(), member).clone(),
            }
        }
    }
}

/*pub fn event(
    name: &String,
    args: Vec<Value>,
    context: Context,
    globals: &mut Globals,
    start_group: Group,
    activate_group: Group,
) {
    match name.as_ref() {
        "Collide" => {
            let block_a_id = match args[0] {
                Value::Block(b) => b,
                _ => panic!("Expected block, got {:?}", args[0]),
            };

            let block_b_id = match args[1] {
                Value::Block(b) => b,
                _ => panic!("Expected block"),
            };

            let group = activate_group;
            let trigger = GDObj {
                obj_id: 1815,
                groups: vec![start_group],
                target: group,
                params: vec![
                    (80, block_a_id.id.to_string()),
                    (95, block_b_id.id.to_string()),
                    (56, "1".to_string()),
                ],
                ..context_trigger(context.clone())
            }
            .context_parameters(context.clone());

            (*globals).obj_list.push(trigger);
        }
        "Touch" => {
            let group = activate_group;
            let trigger = GDObj {
                obj_id: 1595,
                groups: vec![start_group],
                target: group,
                params: vec![(82, "1".to_string()), (81, "1".to_string())],
                ..context_trigger(context.clone())
            }
            .context_parameters(context.clone());

            (*globals).obj_list.push(trigger);
        }

        "TouchEnd" => {
            let group = activate_group;
            let trigger = GDObj {
                obj_id: 1595,
                groups: vec![start_group],
                target: group,
                params: vec![(82, "2".to_string()), (81, "1".to_string())],
                ..context_trigger(context.clone())
            }
            .context_parameters(context.clone());

            (*globals).obj_list.push(trigger);
        }
        "Count" => {
            let item = match args[0] {
                Value::Item(i) => i,
                _ => panic!("Expected item, got {:?}", args[0]),
            };

            let target = match args[1] {
                Value::Number(n) => n,
                _ => panic!("Expected number, got {:?}", args[0]),
            };

            let group = activate_group;
            let trigger = GDObj {
                obj_id: 1611,
                groups: vec![start_group],
                target: group,
                params: vec![
                    (99, "1".to_string()),
                    (104, "1".to_string()), //multi activate
                    (56, "1".to_string()),  //activate group
                    (77, target.to_string()),
                    (80, item.id.to_string()),
                ],
                ..context_trigger(context.clone())
            }
            .context_parameters(context.clone());

            (*globals).obj_list.push(trigger);
        }
        _ => panic!("The event \"{}\" does not exist!", name),
    }
}

pub fn native_func(
    function: ast::Native,
    context: Context,
    globals: &mut Globals,
    start_group: Group,
) -> bool {
    let mut var = function.function;
    let args = function
        .args
        .iter()
        .map(|x| x.value.eval(&context, globals))
        .collect();

    let func_name: String;

    if var.path.is_empty() {
        func_name = match &var.value {
            ast::ValueLiteral::Symbol(s) => s.clone(),
            _ => panic!("Cannot take value as native function name"),
        }
    } else {
        func_name = match var.path[var.path.len() - 1].clone() {
            ast::Path::Member(m) => m,
            _ => panic!("will deprecate"),
        }
    }

    let mut value = Value::Null;
    if var.path.len() > 0 {
        var.path.pop();
        value = var.to_value(&context, globals);
    }

    match value {
        Value::Group(group) => group.native(&func_name, args, context, globals, start_group),
        Value::Func(Func) => Func
            .group
            .native(&func_name, args, context, globals, start_group),
        Value::Color(color) => color.native(&func_name, args, context, globals, start_group),
        Value::Item(item) => item.native(&func_name, args, context, globals, start_group),
        Value::Null => {
            // not called on value
            match func_name.as_str() {
                // group.move(r,g,b,duration,opacity,blending)
                "wait" => {
                    if args.len() < 2 {
                        panic!("Expected 2 arguments")
                    };
                    let duration = match args[0] {
                        Value::Number(n) => n,
                        _ => panic!("Expected number"),
                    };
                    let func = match &args[1] {
                        Value::Func(s) => s.group,
                        _ => panic!("Expected function"),
                    };
                    let trigger = GDObj {
                        obj_id: 1268,
                        target: func,
                        groups: vec![start_group],
                        params: vec![(63, duration.to_string())],
                        ..context_trigger(context.clone())
                    }
                    .context_parameters(context.clone());
                    (*globals).obj_list.push(trigger);
                    true
                }

                "print" => {
                    println!("{:?}", args[0]);
                    true
                }
                _ => false,
            }
        }
        _ => {
            panic!(format!(
                "This value ({:?}) has no native function ascosiated with it!",
                value
            ));
        }
    }
}*/
