///types and functions used by the compiler
use crate::ast;
use crate::builtin::*;
use crate::levelstring::*;

use crate::parser::FileRange;
//use std::boxed::Box;
use std::collections::HashMap;
use std::path::PathBuf;


use smallvec::{SmallVec, smallvec};

use crate::compiler::{compile_scope, import_module, RuntimeError, BUILTIN_STORAGE, NULL_STORAGE, CONTEXT_MAX};


pub type TypeID = u16;
//                                                               This bool is for if this value
//                                                               was implemented in the current module
pub type Implementations = HashMap<TypeID, HashMap<String, (StoredValue, bool)>>;
pub type StoredValue = usize; //index to stored value in globals.stored_values

pub struct ValStorage {
    pub map: HashMap<usize, StoredValData>, //val, fn context, mutable, lifetime
}

#[derive(Debug, Clone)]
pub struct StoredValData {
    pub val: Value,
    pub fn_context: Group,
    pub mutable: bool,
    pub lifetime: u16,
}
/*
LIFETIME:

value gets deleted when lifetime reaches 0
deeper scope => lifetime++
shallower scopr => lifetime--
*/


impl std::ops::Index<usize> for ValStorage {
    type Output = Value;

    fn index(&self, i: usize) -> &Self::Output {
        &self
            .map
            .get(&i)
            .unwrap_or_else(|| panic!("index {} not found", i))
            .val
    }
}

impl std::ops::IndexMut<usize> for ValStorage {
    fn index_mut(&mut self, i: usize) -> &mut Self::Output {
        &mut self.map.get_mut(&i).unwrap().val
    }
}

use std::collections::HashSet;
impl ValStorage {
    pub fn new() -> Self {
        ValStorage {
            map: vec![
                (BUILTIN_STORAGE, StoredValData {val:Value::Builtins, fn_context:Group::new(0), mutable:false, lifetime:1,}),
                (NULL_STORAGE, StoredValData {val:Value::Null, fn_context:Group::new(0), mutable:false, lifetime:1,}),
            ]
            .iter()
            .cloned()
            .collect(),
        }
    }

    pub fn set_mutability(
        &mut self, index: usize, mutable: bool,
    ) {
        if !mutable || !matches!(self[index], Value::Macro(_)) {
            (*self.map.get_mut(&index).unwrap()).mutable = mutable;
        }
       
        
        match self[index].clone() {
            Value::Array(a) => {
                for e in a {
                    self.set_mutability(e, mutable);
                }
            }
            Value::Dict(a) => {
                for (_, e) in a {
                    self.set_mutability(e, mutable);
                }
            }
            Value::Macro(_) => (),
            _ => (),
        };    
    }

    pub fn get_lifetime(&self, index: usize) -> u16 {
        self.map.get(&index).unwrap().lifetime
    }


    pub fn increment_lifetimes(&mut self) {
        for (_, val) in self.map.iter_mut() {
            (*val).lifetime += 1;
        }
    }

    pub fn decrement_lifetimes(&mut self) {
        for (_, val) in self.map.iter_mut() {
            (*val).lifetime -= 1;
        }
    }

    pub fn clean_up(&mut self) {
        let mut to_be_removed = Vec::new();
        for (index, val) in self.map.iter() {
            if val.lifetime == 0 {
                to_be_removed.push(*index);
                //println!("removing value: {:?}", val.0);
            }
        }
        for index in to_be_removed {
            self.map.remove(&index);
        }
    }
    

    pub fn increment_single_lifetime(&mut self, index: usize, amount: u16, already_done: &mut HashSet<usize>) {

        if already_done.get(&index) == None {
            (*already_done).insert(index);
        } else {
            return
        }
        let val = &mut (*self.map.get_mut(&index).expect(&(index.to_string() + " index not found"))).lifetime;
        
        if *val < 10000 - amount {
            *val += amount;
        }
        
        match self[index].clone() {
            Value::Array(a) => {
                for e in a {
                    self.increment_single_lifetime(e, amount, already_done)
                }
            }
            Value::Dict(a) => {
                for (_, e) in a {
                    self.increment_single_lifetime(e, amount, already_done)
                }
            }
            Value::Macro(m) => {
                for (_, e, _, e2) in m.args {
                    if let Some(val) = e {
                        self.increment_single_lifetime(val, amount, already_done)
                    }
                    if let Some(val) = e2 {
                        self.increment_single_lifetime(val, amount, already_done)
                    }
                }

                for (_, v) in m.def_context.variables.iter() {
                    self.increment_single_lifetime(*v, amount, already_done)
                }
            }
            _ => (),
        };
    }
}

pub fn store_value(
    val: Value,
    lifetime: u16,
    globals: &mut Globals,
    context: &Context,
) -> StoredValue {
    let index = globals.val_id;
    let mutable = !matches!(val, Value::Macro(_)); 
    
    
    (*globals)
        .stored_values
        .map
        .insert(index, StoredValData{val, fn_context: context.start_group, mutable, lifetime, });
    (*globals).val_id += 1;
    index
}
pub fn clone_and_get_value(
    index: usize,
    lifetime: u16,
    globals: &mut Globals,
    fn_context: Group,
    constant: bool,
) -> Value {
    let mut old_val = globals.stored_values[index].clone();

    match &mut old_val {
        Value::Array(arr) => {
            old_val = Value::Array(
                arr.iter()
                    .map(|x| clone_value(*x, lifetime, globals, fn_context, constant))
                    .collect(),
            );
        }

        Value::Dict(arr) => {
            old_val = Value::Dict(
                arr.iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            clone_value(*v, lifetime, globals, fn_context, constant),
                        )
                    })
                    .collect(),
            );
        }

        Value::Macro(m) => {
            for arg in &mut m.args {
                if let Some(def_val) = &mut arg.1 {
                    (*def_val) = clone_value(*def_val, lifetime, globals, fn_context, constant);
                }

                if let Some(def_val) = &mut arg.3 {
                    (*def_val) = clone_value(*def_val, lifetime, globals, fn_context, constant);
                }
            }

            // for (_, v) in m.def_context.variables.iter_mut() {
            //     (*v) = clone_value(*v, lifetime, globals, context, constant)
            // }
        }
        _ => (),
    };

    old_val
}


pub fn clone_value(
    index: usize,
    lifetime: u16,
    globals: &mut Globals,
    fn_context: Group,
    constant: bool,
) -> StoredValue {
    let old_val = clone_and_get_value(index, lifetime, globals, fn_context, constant);

    //clone all inner values
    //do the thing
    //bing bang
    //profit
    let new_index = globals.val_id;
    
    (*globals)
        .stored_values
        .map
        .insert(new_index, StoredValData{
            val:old_val, 
            fn_context, 
            mutable:!constant, lifetime, 
            });
    (*globals).val_id += 1;
    new_index
}

pub fn store_const_value(
    val: Value,
    lifetime: u16,
    globals: &mut Globals,
    context: &Context,
) -> StoredValue {
    
    let index = globals.val_id;
    
    (*globals)
        .stored_values
        .map
        .insert(index, StoredValData{val, fn_context:context.start_group, mutable:false, lifetime,});
    (*globals).val_id += 1;
    index
}

pub fn store_val_m(
    val: Value,
    lifetime: u16,
    globals: &mut Globals,
    context: &Context,
    constant: bool,
) -> StoredValue {
    
    let index = globals.val_id;
    
    (*globals)
        .stored_values
        .map
        .insert(index, StoredValData{val, fn_context:context.start_group, mutable:!constant, lifetime, });
    (*globals).val_id += 1;
    index
}

pub type FnIDPtr = usize;

pub type Returns = SmallVec<[(StoredValue, Context); CONTEXT_MAX]>;

#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub enum ImportType {
    Script(PathBuf),
    Lib(String)
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum BreakType {
    Macro,
    Loop,
    ContinueLoop,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct Context {
    pub start_group: Group,
    //pub spawn_triggered: bool,
    pub variables: HashMap<String, StoredValue>,
    //pub self_val: Option<StoredValue>,

    pub func_id: FnIDPtr,

    // info stores the info for the break statement if the context is "broken"
    // broken doesn't mean something is wrong with it, it just means
    // a break statement has been used :)
    pub broken: Option<(CompilerInfo, BreakType)>,


    pub sync_group: usize,
    pub sync_part: SyncPartID,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompilerInfo {
    pub depth: u8,
    pub path: Vec<String>,
    pub current_file: PathBuf,
    pub current_module: String, // empty string means script
    pub pos: FileRange,
    pub includes: Vec<PathBuf>,
}

impl CompilerInfo {
    pub fn new() -> Self {
        CompilerInfo {
            depth: 0,
            path: vec!["main scope".to_string()],
            current_file: PathBuf::new(),
            current_module: String::new(),
            pos: ((0, 0), (0, 0)),
            includes: vec![],
        }
    }
}

impl Context {
    pub fn new() -> Context {
        Context {
            start_group: Group::new(0),
            //spawn_triggered: false,
            variables: HashMap::new(),
            //return_val: Box::new(Value::Null),
            
            //self_val: None,
            func_id: 0,
            broken: None,

            sync_group: 0,
            sync_part: 0,
        }
    }

    pub fn next_fn_id(&self, globals: &mut Globals) -> Context {
        (*globals).func_ids.push(FunctionID {
            parent: Some(self.func_id),
            obj_list: Vec::new(),
            width: None,
        });

        let mut out = self.clone();
        out.func_id = globals.func_ids.len() - 1;
        out
    }

    // pub fn reset_mut_values(&mut self, globals: &mut Globals) {
       
        
    //     for (key, val) in self.variables.clone() {

    //         println!("{}: {:?}", key, globals.stored_values[val]);

    //         let new = clone_value(val, globals.get_lifetime(val), globals, globals.get_fn_context(val), !globals.is_mutable(val));
    //         (*self.variables.get_mut(&key).unwrap()) = new;
        
    //         reset_mut_value(new, globals);
            
            
            
    //     }
    // }

}

// fn reset_mut_value(val: StoredValue, globals: &mut Globals) {
    

//     match globals.stored_values[val].clone() {
//         Value::Array(a) => {
//             for e in a {
//                 reset_mut_value(e, globals);
//             }
//         }
//         Value::Dict(a) => {
//             for (_, e) in a {
//                 reset_mut_value(e, globals);
//             }
//         }
//         Value::Macro(m) => {
//             for (_, e, _, e2) in m.args {
//                 if let Some(val) = e {
//                     reset_mut_value(val, globals);
//                 }
//                 if let Some(val) = e2 {
//                     reset_mut_value(val, globals);
//                 }
//             }

//             for (_, v) in m.def_context.variables.iter() {
//                 reset_mut_value(*v, globals);
//             }
//         }
//         _ => (),
//     };
    
//     if globals.context_change_allowed(val) {
//         globals.stored_values[val] = Value::Null;
//     }
// }
// pub fn compare_contexts(context1: Context, context2: Context, globals: &mut Globals) -> bool {
//     // returns true if the contexts are equal/mergable

// }

//will merge one set of context, returning false if no mergable contexts were found
pub fn merge_contexts(contexts: &mut SmallVec<[Context; CONTEXT_MAX]>, globals: &mut Globals) -> bool {
    
    let mut mergable_ind = Vec::<usize>::new();
    let mut ref_c = 0;
    loop {
        if ref_c >= contexts.len() {
            return false;
        }
        for (i, c) in contexts.iter().enumerate() {
            if i == ref_c {
                continue;
            }
            let ref_c = &contexts[ref_c];

            if (ref_c.broken == None) != (c.broken == None) {
                continue;
            }
            let mut not_eq = false;

            //check variables are equal
            for (key, val) in &c.variables {
                if globals.stored_values[ref_c.variables[key]] != globals.stored_values[*val] {
                    not_eq = true;
                    break;
                }
            }
            if not_eq {
                continue;
            }
            //check implementations are equal
            // for (key, val) in &c.implementations {
            //     for (key2, val) in val {
            //         if globals.stored_values[ref_c.implementations[key][key2]] != globals.stored_values[*val] {
            //             not_eq = true;
            //             break;
            //         }
            //     }
            // }
            // if not_eq {
            //     continue;
            // }

            //everything is equal, add to list
            mergable_ind.push(i);
        }
        if mergable_ind.is_empty() {
            ref_c += 1;
        } else {
            break
        }
    }

    let new_group = Group::next_free(&mut globals.closed_groups);
    //add spawn triggers
    let mut add_spawn_trigger = |context: &Context| {
        let mut params = HashMap::new();
        params.insert(
            51,
            ObjParam::Group(new_group),
        );
        params.insert(1, ObjParam::Number(1268.0));
        (*globals).trigger_order += 1;

        (*globals).func_ids[context.func_id].obj_list.push(
            (GDObj {
                params,

                ..context_trigger(&context,&mut globals.uid_counter)
            }
            .context_parameters(&context),globals.trigger_order)
        )
    };
    add_spawn_trigger(&contexts[ref_c]);
    for i in mergable_ind.iter() {
        add_spawn_trigger(&contexts[*i])
    }
    
    (*contexts)[ref_c].start_group = new_group;
    (*contexts)[ref_c].next_fn_id(globals);
    
    for i in mergable_ind.iter().rev() {
        (*contexts).swap_remove(*i);
    }

    true
    
}

#[derive(Clone, Debug, PartialEq)]
pub struct Macro {
    //             name         default val      tag          pattern
    pub args: Vec<(String, Option<StoredValue>, ast::Tag, Option<StoredValue>)>,
    pub def_context: Context,
    pub def_file: PathBuf,
    pub body: Vec<ast::Statement>,
    pub tag: ast::Tag,
}
#[derive(Clone, Debug, PartialEq)]
pub struct TriggerFunction {
    pub start_group: Group,
    //pub all_groups: Vec<Group>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Pattern {
    Type(TypeID),
    Array(Vec<Pattern>),
    Either(Box<Pattern>, Box<Pattern>),
}

#[derive(Clone, Debug, PartialEq)]

pub enum Value {
    Group(Group),
    Color(Color),
    Block(Block),
    Item(Item),
    Number(f64),
    Bool(bool),
    TriggerFunc(TriggerFunction),
    Dict(HashMap<String, StoredValue>),
    Macro(Box<Macro>),
    Str(String),
    Array(Vec<StoredValue>),
    Obj(Vec<(u16, ObjParam)>, ast::ObjectMode),
    Builtins,
    BuiltinFunction(String),
    TypeIndicator(TypeID),
    Range(i32, i32, usize), //start, end, step
    Pattern(Pattern),
    Null,
}

pub fn value_equality(val1: StoredValue, val2: StoredValue, globals: &Globals) -> bool {
    match (&globals.stored_values[val1], &globals.stored_values[val2]) {
        (Value::Array(a1), Value::Array(a2)) => {
            if a1.len() != a2.len() {
                return false
            }

            for i in 0..a1.len() {
                if !value_equality(a1[i], a2[i], globals) {
                    return false
                }
            }
            true
        }
        (Value::Dict(d1), Value::Dict(d2)) => {
            if d1.len() != d2.len() {
                return false
            }

            for key in d1.keys() {
                if let Some(val1) = d2.get(key) {
                    if let Some(val2) = d1.get(key) {
                        if !value_equality(*val1, *val2, globals) {
                            return false
                        }
                    } else {
                        unreachable!()
                    }
                
                } else {
                    return false
                }
            }
            true
        }
        (a, b) => a == b,
    }
}
 
impl Value {
    //numeric representation of value
    pub fn to_num(&self, globals: &Globals) -> TypeID {
        match self {
            Value::Group(_) => 0,
            Value::Color(_) => 1,
            Value::Block(_) => 2,
            Value::Item(_) => 3,
            Value::Number(_) => 4,
            Value::Bool(_) => 5,
            Value::TriggerFunc(_) => 6,
            Value::Dict(d) => match d.get(TYPE_MEMBER_NAME) {
                Some(member) => match globals.stored_values[*member as usize] {
                    Value::TypeIndicator(t) => t,
                    _ => unreachable!(),
                },

                None => 7,
            },
            Value::Macro(_) => 8,
            Value::Str(_) => 9,
            Value::Array(_) => 10,
            Value::Obj(_, mode) => match mode {
                ast::ObjectMode::Object => 11,
                ast::ObjectMode::Trigger => 16,
            },
            Value::Builtins => 12,
            Value::BuiltinFunction(_) => 13,
            Value::TypeIndicator(_) => 14,
            Value::Null => 15,
            Value::Range(_, _, _) => 17,
            Value::Pattern(_) => 18,
        }
    }



    pub fn matches_pat(&self, pat_val: &Value, info: &CompilerInfo, globals: &mut Globals, context: &Context) -> Result<bool, RuntimeError> {
        let pat = if let Value::Pattern(p) = convert_type(pat_val, 18, info, globals, context)? {p} else {unreachable!()};
        match pat {
            Pattern::Either(p1, p2) => Ok(self.matches_pat(&Value::Pattern(*p1), info, globals, context)? || self.matches_pat(&Value::Pattern(*p2), info,globals, context)?),
            Pattern::Type(t) => Ok(self.to_num(globals) == t),
            Pattern::Array(a_pat) => {
                if let Value::Array(a_val) = self {
                    match a_pat.len() {
                        0 => Ok(true),
    
                        1 => {
                            for el in a_val {
                                let val = globals.stored_values[*el].clone();
                                if !val.matches_pat(&Value::Pattern(a_pat[0].clone()), info, globals, context)? {
                                    return Ok(false)
                                }
                            }
                            Ok(true)
                        }

                        _ => Err(RuntimeError::RuntimeError {
                            message: String::from("arrays with multiple elements cannot be used as patterns (yet)"),
                            info: info.clone(),
                        })
                    }
                } else {
                    Ok(false)
                }
                
            }
        }
    }
}

//copied from https://stackoverflow.com/questions/59401720/how-do-i-find-the-key-for-a-value-in-a-hashmap
pub fn find_key_for_value(map: &HashMap<String, (u16, PathBuf, (usize, usize))>, value: u16) -> Option<&String> {
    map.iter()
        .find_map(|(key, val)| if val.0 == value { Some(key) } else { None })
}






pub fn convert_type(
    val: &Value,
    typ: TypeID,
    info: &CompilerInfo,
    globals: &mut Globals,
    context: &Context,
) -> Result<Value, RuntimeError> {

    if val.to_num(globals) == typ {
        return Ok(val.clone())
    }

    if typ == 9 {
        return Ok(Value::Str(val.to_str(globals)));
    }

    Ok(match val {
        Value::Number(n) => match typ {
            0 => Value::Group(Group::new(*n as u16)),
            1 => Value::Color(Color::new(*n as u16)),
            2 => Value::Block(Block::new(*n as u16)),
            3 => Value::Item(Item::new(*n as u16)),
            4 => Value::Number(*n),
            5 => Value::Bool(*n != 0.0),

            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "Number can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }
        },

        Value::Group(g) => match typ {
            
            4 => Value::Number(match g.id {
                ID::Specific(n) => n as f64,
                _ => return Err(RuntimeError::RuntimeError {
                    message: "This group isn\'t known at this time, and can therefore not be converted to a number!".to_string(),
                    info: info.clone(),
                })
            }),
            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "Group can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }
        },

        Value::Color(c) => match typ {
            
            4 => Value::Number(match c.id {
                ID::Specific(n) => n as f64,
                _ => return Err(RuntimeError::RuntimeError {
                    message: "This color isn\'t known at this time, and can therefore not be converted to a number!".to_string(),
                    info: info.clone(),
                })
            }),
            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "Color can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }
        },

        Value::Block(b) => match typ {
            
            4 => Value::Number(match b.id {
                ID::Specific(n) => n as f64,
                _ => return Err(RuntimeError::RuntimeError {
                    message: "This block ID isn\'t known at this time, and can therefore not be converted to a number!".to_string(),
                    info: info.clone(),
                })
            }),
            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "Block ID can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }
        },

        Value::Item(i) => match typ {
            
            4 => Value::Number(match i.id {
                ID::Specific(n) => n as f64,
                _ => return Err(RuntimeError::RuntimeError {
                    message: "This item ID isn\'t known at this time, and can therefore not be converted to a number!".to_string(),
                    info: info.clone(),
                })
            }),
            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "Item ID can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }
        },

        Value::Bool(b) => match typ {
            
            4 => Value::Number(if *b { 1.0 } else { 0.0 }),
            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "Boolean can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }
        },

        Value::TriggerFunc(f) => match typ {
            
            0 => Value::Group(f.start_group),
            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "Trigger function can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }
        },

        Value::Range(start, end, step) => match typ {
            10 => {
                Value::Array(if start < end { 
                    (*start..*end).step_by(*step).map(|x| 
                        store_value(Value::Number(x as f64), 1, globals, &context)).collect::<Vec<StoredValue>>() 
                } else { 
                    (*end..*start).step_by(*step).rev().map(|x| 
                        store_value(Value::Number(x as f64), 1, globals, &context)).collect::<Vec<StoredValue>>()
                })
            },
            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "Range can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }
        },

        Value::Str(s) => match typ {
            4 => {
                let out: std::result::Result<f64, _> = s.parse();
                match out {
                    Ok(n) => Value::Number(n),
                    _ => {
                        return Err(RuntimeError::RuntimeError {
                            message: format!("Cannot convert '{}' to @number", s),
                            info: info.clone()
                        })
                    }
                }
            },
            10 => {
                Value::Array(s.chars().map(|x| store_value(Value::Str(x.to_string()), 1, globals, &context)).collect::<Vec<StoredValue>>())
            },
            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "String can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }
        },

        Value::Array(arr) => match typ {
            18 => {
                // pattern
                let mut new_vec = Vec::new();
                for el in arr {
                    new_vec.push(match globals.stored_values[*el].clone() {
                        Value::Pattern(p) => p,
                        a => if let Value::Pattern(p) = convert_type(&a, 18, info, globals, context)? {
                            p
                        } else {
                            unreachable!()
                        },
                    })
                }
                Value::Pattern(Pattern::Array(new_vec))
            }

            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "Array can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }

        }
        Value::TypeIndicator(t) =>  match typ {
            18 => {
                
                Value::Pattern(Pattern::Type(*t))
            }

            _ => {
                return Err(RuntimeError::RuntimeError {
                    message: format!(
                        "Type-Indicator can't be converted to '{}'!",
                        find_key_for_value(&globals.type_ids, typ).unwrap()
                    ),
                    info: info.clone(),
                })
            }
        }

        _ => {
            return Err(RuntimeError::RuntimeError {
                message: format!(
                    "'{}' can't be converted to '{}'!",
                    find_key_for_value(&globals.type_ids, typ).unwrap(), find_key_for_value(&globals.type_ids, val.to_num(globals)).unwrap()
                ),
                info: info.clone(),
            })
        }
    })
}

//use std::fmt;

const MAX_DICT_EL_DISPLAY: u16 = 10;

impl Value {
    
    pub fn to_str(&self, globals: &Globals) -> String {
        match self {
            Value::Group(g) => {
                (if let ID::Specific(id) = g.id {
                    id.to_string()
                } else {
                    "?".to_string()
                }) + "g"
            }
            Value::Color(c) => {
                (if let ID::Specific(id) = c.id {
                    id.to_string()
                } else {
                    "?".to_string()
                }) + "c"
            }
            Value::Block(b) => {
                (if let ID::Specific(id) = b.id {
                    id.to_string()
                } else {
                    "?".to_string()
                }) + "b"
            }
            Value::Item(i) => {
                (if let ID::Specific(id) = i.id {
                    id.to_string()
                } else {
                    "?".to_string()
                }) + "i"
            }
            Value::Number(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::TriggerFunc(_) => "!{ /* trigger function */ }".to_string(),
            Value::Range(start, end, stepsize) => {
                if *stepsize != 1 {
                    format!("{}..{}..{}", start, stepsize, end)
                } else {
                    format!("{}..{}", start, end)
                }
            }
            Value::Dict(dict_in) => {
                let mut out = String::new();
                let mut count = 0;
                let mut d = dict_in.clone();
                if let Some(n) = d.get(TYPE_MEMBER_NAME) {
                    let val = &globals.stored_values[*n];
                    out += &val.to_str(globals);
                    d.remove(TYPE_MEMBER_NAME);
                    out += "::";
                }
                out += "{";
                let mut d_iter = d.iter();
                for (key, val) in &mut d_iter {
                    

                    if count > MAX_DICT_EL_DISPLAY {
                        let left = d_iter.count();
                        if left > 0 {
                            out += &format!("... ({} more)  ", left);
    
                        }
                        break;
                        
                    }
                    count += 1;
                    let stored_val = (*globals).stored_values[*val as usize].to_str(globals);
                    out += &format!("{}: {},", key, stored_val);
                }
                if !d.is_empty() {
                    out.pop();
                }
                

                out += "}"; //why do i have to do this twice? idk

                out
            }
            Value::Macro(m) => {
                let mut out = String::from("(");
                if !m.args.is_empty() {
                    for arg in m.args.iter() {
                        out += &arg.0;
                        if let Some(val) = arg.3 {
                            out += &format!(
                                ": {}",
                                globals.stored_values[val].to_str(globals),
                            )
                        };
                        if let Some(val) = arg.1 {
                            out += &format!(" = {}", globals.stored_values[val].to_str(globals))
                        };
                        out += ", ";
                    }
                    out.pop();
                    out.pop();
                }
                out + ") { /* code omitted */ }"
            }
            Value::Str(s) => s.clone(),
            Value::Array(a) => {
                if a.is_empty() {
                    "[]".to_string()
                } else {
                    let mut out = String::from("[");
                    for val in a {
                        out += &globals.stored_values[*val].to_str(globals);
                        out += ",";
                    }
                    out.pop();
                    out += "]";

                    out
                }
            }
            Value::Obj(o, _) => {
                let mut out = String::new();
                for (key, val) in o {
                    out += &format!("{},{},", key, val);
                }
                out.pop();
                out += ";";
                out
            }
            Value::Builtins => "SPWN".to_string(),
            Value::BuiltinFunction(n) => format!("<built-in-function: {}>", n),
            Value::Null => "Null".to_string(),
            Value::TypeIndicator(id) => format!(
                "@{}",
                match find_key_for_value(&globals.type_ids, *id) {
                    Some(name) => name,
                    None => "[TYPE NOT FOUND]",
                }
            ),

            Value::Pattern(p) => match p {
                Pattern::Type(t) => Value::TypeIndicator(*t).to_str(globals),
                Pattern::Either(p1, p2) => format!("{} | {}", Value::Pattern(*p1.clone()).to_str(globals), Value::Pattern(*p2.clone()).to_str(globals)),
                Pattern::Array(a) => if a.is_empty() {
                    "[]".to_string()
                } else {
                    let mut out = String::from("[");
                    for p in a {
                        out += &Value::Pattern(p.clone()).to_str(globals);
                        out += ",";
                    }
                    out.pop();
                    out += "]";

                    out
                },
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FunctionID {
    pub parent: Option<usize>, //index of parent id, if none it is a top-level id
    pub width: Option<u32>,    //width of this id, is none when its not calculated yet
    //pub name: String,          //name of this id, used for the label
    pub obj_list: Vec<(GDObj, usize)>, //list of objects in this function id, + their order id
}

pub type SyncPartID = usize;
pub struct SyncGroup {
    parts: Vec<SyncPartID>,
    groups_used: Vec<ArbitraryID>, // groups that are already used by this sync group, and can be reused in later parts
}

pub struct Globals {
    //counters for arbitrary groups
    pub closed_groups: u16,
    pub closed_colors: u16,
    pub closed_blocks: u16,
    pub closed_items: u16,

    pub path: PathBuf,

    pub lowest_y: HashMap<u32, u16>,
    pub stored_values: ValStorage,
    pub val_id: usize,

    pub type_ids: HashMap<String, (u16, PathBuf, (usize, usize))>,
    pub type_id_count: u16,

    pub func_ids: Vec<FunctionID>,
    pub objects: Vec<GDObj>,

    pub prev_imports: HashMap<ImportType, (Value, Implementations)>,

    pub trigger_order: usize,

    pub uid_counter: usize,
    pub implementations: Implementations,

    pub sync_groups: Vec<SyncGroup>,
}

impl Globals {
    pub fn get_val_fn_context(
        &self,
        p: StoredValue,
        info: CompilerInfo,
    ) -> Result<Group, RuntimeError> {
        match self.stored_values.map.get(&p) {
            Some(val) => Ok(val.fn_context),
            None => Err(RuntimeError::RuntimeError {
                message: "Pointer points to no data!".to_string(),
                info,
            }),
        }
    }
    pub fn is_mutable(&self, p: StoredValue) -> bool {
        match self.stored_values.map.get(&p) {
            Some(val) => val.mutable,
            None => unreachable!(),
        }
    }

    pub fn can_mutate(&self, p: StoredValue) -> bool {
        self.is_mutable(p)
    }

    pub fn get_fn_context(&self, p: StoredValue) -> Group {
        match self.stored_values.map.get(&p) {
            Some(val) => val.fn_context,
            None => unreachable!(),
        }
    }

    pub fn get_lifetime(&self, p: StoredValue) -> u16 {
        match self.stored_values.map.get(&p) {
            Some(val) => val.lifetime,
            None => unreachable!(),
        }
    }

    

    pub fn get_type_str(&self, p: StoredValue) -> String {
        let val = &self.stored_values[p];
        let typ = match val {
            Value::Dict(d) => {
                if let Some(s) = d.get(TYPE_MEMBER_NAME) {
                    match self.stored_values[*s] {
                        Value::TypeIndicator(t) => t,
                        _ => unreachable!(),
                    }
                } else {
                    val.to_num(self)
                }
            }
            _ => val.to_num(self),
        };
        find_key_for_value(&self.type_ids, typ).unwrap().clone()
    }
}

impl Globals {
    pub fn new(path: PathBuf) -> Self {
        let storage = ValStorage::new();
        let mut globals = Globals {
            closed_groups: 0,
            closed_colors: 0,
            closed_blocks: 0,
            closed_items: 0,
            path,

            lowest_y: HashMap::new(),

            type_ids: HashMap::new(),

            prev_imports: HashMap::new(),
            type_id_count: 0,
            trigger_order: 0,
            uid_counter: 0,

            val_id: storage.map.len(),
            stored_values: storage,
            func_ids: vec![FunctionID {
                parent: None,
                width: None,
                obj_list: Vec::new(),
            }],
            objects: Vec::new(),
            implementations: HashMap::new(),
            sync_groups: vec![SyncGroup {
                parts: vec![0],
                groups_used: Vec::new()
            }]
        };

        

        let mut add_type = |name: &str, id: u16| {
            globals.type_ids.insert(String::from(name), (id, PathBuf::new(), (0,0)))
        };

        add_type("group", 0);
        add_type("color", 1);
        add_type("block", 2);
        add_type("item", 3);
        add_type("number", 4);
        add_type("bool", 5);
        add_type("trigger_function", 6);
        add_type("dictionary", 7);
        add_type("macro", 8);
        add_type("string", 9);
        add_type("array", 10);
        add_type("object", 11);
        add_type("spwn", 12);
        add_type("builtin", 13);
        add_type("type_indicator", 14);
        add_type("NULL", 15);
        add_type("trigger", 16);
        add_type("range", 17);
        add_type("pattern", 18);
        add_type("object_key", 19);
        add_type("epsilon", 20);



        globals.type_id_count = globals.type_ids.len() as u16;

        globals
    }
}

fn handle_operator(
    value1: StoredValue,
    value2: StoredValue,
    macro_name: &str,
    context: &Context,
    globals: &mut Globals,
    info: &CompilerInfo,
) -> Result<Returns, RuntimeError> {
    Ok(
        if let Some(val) =
            globals.stored_values[value1]
                .clone()
                .member(macro_name.to_string(), &context, globals)
        {
            if let Value::Macro(m) = globals.stored_values[val].clone() {
                
                if m.args.is_empty() {
                    return Err(RuntimeError::RuntimeError {
                        message: String::from("Expected at least one argument in operator macro"),
                        info: info.clone(),
                    });
                }
                let val2 = globals.stored_values[value2].clone();

                if let Some(target_typ) = m.args[0].3 {
                    let pat = &globals.stored_values[target_typ].clone();

                    if  !val2.matches_pat(pat, &info, globals, context)? {
                        //if types dont match, act as if there is no macro at all
                        return Ok(smallvec![(
                            store_value(
                                built_in_function(
                                    macro_name,
                                    vec![value1, value2],
                                    info.clone(),
                                    globals,
                                    &context,
                                )?,
                                1,
                                globals,
                                &context,
                            ),
                            context.clone(),
                        )]);
                    }
                }

                let (values, _) = execute_macro(
                    (
                        *m,
                        //copies argument so the original value can't be mutated
                        //prevents side effects and shit
                        vec![ast::Argument::from(clone_value(value2, 1, globals, context.start_group, false))],
                    ),
                    context,
                    globals,
                    value1,
                    info.clone(),
                )?;
                values
            } else {
                smallvec![(
                    store_value(
                        built_in_function(
                            macro_name,
                            vec![value1, value2],
                            info.clone(),
                            globals,
                            &context,
                        )?,
                        1,
                        globals,
                        &context,
                    ),
                    context.clone(),
                )]
            }
        } else {
            smallvec![(
                store_value(
                    built_in_function(macro_name, vec![value1, value2], info.clone(), globals, &context)?,
                    1,
                    globals,
                    &context,
                ),
                context.clone(),
            )]
        },
    )
}

pub fn convert_to_int(num: f64, info: &CompilerInfo) -> Result<i32, RuntimeError> {
    let rounded = num.round();
    if (num - rounded).abs() > 0.000000001 {
        return Err(RuntimeError::RuntimeError {
            message: format!("expected integer, found {}", num),
            info: info.clone(),
        });
    }
    Ok(rounded as i32)
}

impl ast::Expression {
    pub fn eval(
        &self,
        context: &Context,
        globals: &mut Globals,
        mut info: CompilerInfo,
        constant: bool,
    ) -> Result<(Returns, Returns), RuntimeError> {
        //second returns is in case there are any values in the expression that includes a return statement
        let mut vals = self.values.iter();
        let first = vals.next().unwrap();
        let first_value = first.to_value(context.clone(), globals, info.clone(), constant)?;
        let mut acum = first_value.0;
        let mut inner_returns = first_value.1;

        let mut start_pos = first.pos.0;

        if self.operators.is_empty() {
            //if only variable
            return Ok((acum, inner_returns));
        }

        for (i, var) in vals.enumerate() {
            let mut new_acum: Returns = SmallVec::new();
            let end_pos = var.pos.1;
            info.pos = (start_pos, end_pos);
            //every value in acum will be operated with the value of var in the corresponding context
            for (acum_val, c) in acum {
                use ast::Operator::*;

                //only eval the first one on Or and And
                let (or_overwritten, and_overwritten) = 
                    if let Some(imp) = globals.implementations.get(&5) {
                        (imp.get("_or_") != None, imp.get("_and_") != None)
                    } else {
                        (false, false)
                    };
                if self.operators[i] == Or && !or_overwritten && globals.stored_values[acum_val] == Value::Bool(true) {
                    let stored = store_const_value(Value::Bool(true), 1, globals, &c);
                    new_acum.push((stored, c));
                    continue;
                } else if self.operators[i] == And && !and_overwritten && globals.stored_values[acum_val] == Value::Bool(false) {
                    let stored = store_const_value(Value::Bool(false), 1, globals, &c);
                    new_acum.push((stored, c));
                    continue;
                }

                //what the value in acum becomes
                let evaled = var.to_value(c, globals, info.clone(), constant)?;
                inner_returns.extend(evaled.1);

                

                for (val, c2) in &evaled.0 {
                    //let val_fn_context = globals.get_val_fn_context(val, info.clone());
                    let vals: Returns = match self.operators[i] {
                        Or => handle_operator(acum_val, *val, "_or_", c2, globals, &info)?,
                        And => handle_operator(acum_val, *val, "_and_", c2, globals, &info)?,
                        More => handle_operator(
                            acum_val,
                            *val,
                            "_more_than_",
                            c2,
                            globals,
                            &info,
                        )?,
                        Less => handle_operator(
                            acum_val,
                            *val,
                            "_less_than_",
                            c2,
                            globals,
                            &info,
                        )?,
                        MoreOrEqual => handle_operator(
                            acum_val,
                            *val,
                            "_more_or_equal_",
                            c2,
                            globals,
                            &info,
                        )?,
                        LessOrEqual => handle_operator(
                            acum_val,
                            *val,
                            "_less_or_equal_",
                            c2,
                            globals,
                            &info,
                        )?,
                        Slash => handle_operator(
                            acum_val,
                            *val,
                            "_divided_by_",
                            c2,
                            globals,
                            &info,
                        )?,

                        IntDividedBy => {
                            handle_operator(acum_val, *val, "_intdivided_by_", c2, globals, &info)?
                        },

                        Star => {
                            handle_operator(acum_val, *val, "_times_", c2, globals, &info)?
                        }

                        Modulo => {
                            handle_operator(acum_val, *val, "_mod_", c2, globals, &info)?
                        }

                        Power => {
                            handle_operator(acum_val, *val, "_pow_", c2, globals, &info)?
                        }
                        Plus => {
                            handle_operator(acum_val, *val, "_plus_", c2, globals, &info)?
                        }
                        Minus => {
                            handle_operator(acum_val, *val, "_minus_", c2, globals, &info)?
                        }
                        Equal => {
                            handle_operator(acum_val, *val, "_equal_", c2, globals, &info)?
                        }
                        NotEqual => handle_operator(
                            acum_val,
                            *val,
                            "_not_equal_",
                            c2,
                            globals,
                            &info,
                        )?,
                        
                        Either => handle_operator(acum_val,
                            *val,
                            "_either_",
                            c2,
                            globals,
                            &info
                        )?,
                        Range => handle_operator(acum_val,
                            *val,
                            "_range_",
                            c2,
                            globals,
                            &info
                        )?,
                        //MUTABLE ONLY
                        //ADD CHECk
                        Assign => {
                            handle_operator(acum_val, *val, "_assign_", c2, globals, &info)?
                        },

                        Swap => {
                            handle_operator(acum_val, *val, "_swap_", c2, globals, &info)?
                        },

                        As => handle_operator(acum_val, *val, "_as_", c2, globals, &info)?,

                        Has => handle_operator(acum_val, *val, "_has_", c2, globals, &info)?,

                        Add => handle_operator(acum_val, *val, "_add_", c2, globals, &info)?,

                        Subtract => handle_operator(
                            acum_val,
                            *val,
                            "_subtract_",
                            c2,
                            globals,
                            &info,
                        )?,

                        Multiply => handle_operator(
                            acum_val,
                            *val,
                            "_multiply_",
                            c2,
                            globals,
                            &info,
                        )?,

                        Exponate => handle_operator(
                            acum_val,
                            *val,
                            "_exponate_",
                            c2,
                            globals,
                            &info,
                        )?,

                        Modulate => handle_operator(
                            acum_val,
                            *val,
                            "_modulate_",
                            c2,
                            globals,
                            &info,
                        )?,

                        Divide => {
                            handle_operator(acum_val, *val, "_divide_", c2, globals, &info)?
                        },

                        IntDivide => {
                            handle_operator(acum_val, *val, "_intdivide_", c2, globals, &info)?
                        },                 

                    };
                    new_acum.extend(vals);
                }
            }
            acum = new_acum;
            start_pos = var.pos.0;
        }
        Ok((acum, inner_returns))
    }


}

pub fn execute_macro(
    (m, args): (Macro, Vec<ast::Argument>),
    context: &Context,
    globals: &mut Globals,
    parent: StoredValue,
    info: CompilerInfo,
) -> Result<(Returns, Returns), RuntimeError> {
    let mut inner_inner_returns = SmallVec::new();
    let mut new_contexts: SmallVec<[Context; CONTEXT_MAX]> = SmallVec::new();
    if !m.args.is_empty() {
        // second returns is for any compound statements in the args
        let (evaled_args, inner_returns) = all_combinations(
            args.iter().map(|x| x.value.clone()).collect(),
            context,
            globals,
            info.clone(),
            true,
        )?;
        inner_inner_returns.extend(inner_returns);

        for (arg_values, mut new_context) in evaled_args {
            new_context.variables = m.def_context.variables.clone();
            let mut new_variables: HashMap<String, StoredValue> = HashMap::new();

            //parse each argument given into a local macro variable
            //index of arg if no arg is specified
            let mut def_index = if m.args[0].0 == "self" { 1 } else { 0 };
            for (i, arg) in args.iter().enumerate() {
                match &arg.symbol {
                    Some(name) => {
                        let arg_def = m.args.iter().enumerate().find(|e| e.1 .0 == *name);
                        if let Some((_arg_i, arg_def)) = arg_def {
                            //type check!!
                            //maybe make type check function
                            if let Some(t) = arg_def.3 {
                                let val = globals.stored_values[arg_values[i]].clone();
                                let pat = globals.stored_values[t].clone();

                                if !val.matches_pat(&pat, &info, globals, context)? {
                                    return Err(RuntimeError::TypeError {
                                        expected: pat.to_str(globals),
                                        found: val.to_str(globals),
                                        info,
                                    });
                                }
                            };

                            new_variables.insert(name.clone(), arg_values[i]);
                        } else {
                            return Err(RuntimeError::UndefinedErr {
                                undefined: name.clone(),
                                info,
                                desc: "macro argument".to_string(),
                            });
                        }
                    }
                    None => {
                        if (def_index) > m.args.len() - 1 {
                            return Err(RuntimeError::RuntimeError {
                                message: "Too many arguments!".to_string(),
                                info,
                            });
                        }

                        //type check!!
                        if let Some(t) = m.args[def_index].3 {
                            let val = globals.stored_values[arg_values[i]].clone();
                            let pat = globals.stored_values[t].clone();

                            if !val.matches_pat(&pat, &info, globals, context)? {
                                return Err(RuntimeError::TypeError {
                                    expected: pat.to_str(globals),
                                    found: val.to_str(globals),
                                    info,
                                });
                            }
                        };

                        new_variables.insert(
                            m.args[def_index].0.clone(),
                            clone_value(arg_values[i], 1, globals, context.start_group, true),
                        );
                        def_index += 1;
                    }
                }
            }
            //insert defaults and check non-optional arguments
            let mut m_args_iter = m.args.iter();
            if m.args[0].0 == "self" {
                if globals.stored_values[parent] == Value::Null {
                    return Err(RuntimeError::RuntimeError {
                        message: "
This macro requires a parent (a \"self\" value), but it seems to have been called alone (or on a null value).
Should be used like this: value.macro(arguments)".to_string(), info
                    });
                }
                //self doesn't need to be cloned, as it is a referance (kinda)
                new_context.variables.insert("self".to_string(), parent);
                m_args_iter.next();
            }
            for arg in m_args_iter {
                if !new_variables.contains_key(&arg.0) {
                    match &arg.1 {
                        Some(default) => {
                            new_variables.insert(
                                arg.0.clone(),
                                clone_value(*default, 1, globals, context.start_group, true),
                            );
                        }

                        None => {
                            return Err(RuntimeError::RuntimeError {
                                message: format!(
                                    "Non-optional argument '{}' not satisfied!",
                                    arg.0
                                ),
                                info,
                            })
                        }
                    }
                }
            }

            new_context.variables.extend(new_variables);

            new_contexts.push(new_context);
        }
    } else {
        let mut new_context = context.clone();
        new_context.variables = m.def_context.variables.clone();
        /*let mut new_variables: HashMap<String, StoredValue> = HashMap::new();

        if m.args[0].0 == "self" {
            new_variables.insert("self".to_string(), store_value(parent.clone(), globals));
        }

        new_context.variables.extend(new_variables);*/

        new_contexts.push(new_context);
    }
    let mut new_info = info;
    new_info.current_file = m.def_file;
    let mut compiled = compile_scope(&m.body, new_contexts, globals, new_info)?;

    // stop break chain
    for c in &mut compiled.0 {
        if let Some((i, BreakType::Loop)) = &(*c).broken {
            return Err(RuntimeError::RuntimeError {
                message: "break statement is never used".to_string(),
                info: i.clone(),
            });
        }
        (*c).broken = None;
    }

    let returns = if compiled.1.is_empty() {
        compiled.0.iter().map(|x| (1, x.clone())).collect()
    } else {
        compiled.1
    };

    Ok((
        returns
            .iter()
            .map(|x| {
                //set mutable to false
                (*globals.stored_values.map.get_mut(&x.0).unwrap()).mutable = false;
                (
                    x.0,
                    Context {
                        variables: context.variables.clone(),
                        ..x.1.clone()
                    },
                )
            })
            .collect(),
        inner_inner_returns,
    ))
}
type ReturnsList = Vec<(Vec<StoredValue>, Context)>;
fn all_combinations(
    a: Vec<ast::Expression>,
    context: &Context,
    globals: &mut Globals,
    info: CompilerInfo,
    constant: bool,
) -> Result<(ReturnsList, Returns), RuntimeError> {
    let mut out = ReturnsList::new();
    let mut inner_returns = Returns::new();
    if a.is_empty() {
        //if there are so value, there is only one combination
        out.push((Vec::new(), context.clone()));
    } else {
        let mut a_iter = a.iter();
        //starts with all the combinations of the first expr
        let (start_values, start_returns) =
            a_iter
                .next()
                .unwrap()
                .eval(context, globals, info.clone(), constant)?;
        out.extend(start_values.iter().map(|x| (vec![x.0], x.1.clone())));
        inner_returns.extend(start_returns);
        //for the rest of the expressions
        for expr in a_iter {
            //all the new combinations end up in this
            let mut new_out: Vec<(Vec<StoredValue>, Context)> = Vec::new();
            //run through all the lists in out
            for (inner_arr, c) in out.iter() {
                //for each one, run through all the returns in that context
                let (values, returns) = expr.eval(c, globals, info.clone(), constant)?;
                inner_returns.extend(returns);
                for (v, c2) in values.iter() {
                    //push a new list with each return pushed to it
                    new_out.push((
                        {
                            let mut new_arr = inner_arr.clone();
                            new_arr.push(*v);
                            new_arr
                        },
                        c2.clone(),
                    ));
                }
            }
            //set out to this new one and repeat
            out = new_out;
        }
    }
    Ok((out, inner_returns))
}
pub fn eval_dict(
    dict: Vec<ast::DictDef>,
    context: &Context,
    globals: &mut Globals,
    info: CompilerInfo,
    constant: bool,
) -> Result<(Returns, Returns), RuntimeError> {
    let mut inner_returns = Returns::new();
    let (evaled, returns) = all_combinations(
        dict.iter()
            .map(|def| match def {
                ast::DictDef::Def(d) => d.1.clone(),
                ast::DictDef::Extract(e) => e.clone(),
            })
            .collect(),
        context,
        globals,
        info.clone(),
        constant,
    )?;
    inner_returns.extend(returns);
    let mut out = Returns::new();
    for expressions in evaled {
        let mut dict_out: HashMap<String, StoredValue> = HashMap::new();
        for (expr_index, def) in dict.iter().enumerate() {
            match def {
                ast::DictDef::Def(d) => {
                    dict_out.insert(d.0.clone(), expressions.0[expr_index]);
                }
                ast::DictDef::Extract(_) => {
                    dict_out.extend(
                        match globals.stored_values[expressions.0[expr_index]].clone() {
                            Value::Dict(d) => d.clone(),
                            a => {
                                return Err(RuntimeError::RuntimeError {
                                    message: format!(
                                        "Cannot extract from this value: {}",
                                        a.to_str(globals)
                                    ),
                                    info,
                                })
                            }
                        },
                    );
                }
            };
        }
        out.push((
            store_value(Value::Dict(dict_out), 1, globals, &context),
            expressions.1,
        ));
    }
    Ok((out, inner_returns))
}

impl ast::Variable {
    pub fn to_value(
        &self,
        mut context: Context,
        globals: &mut Globals,
        mut info: CompilerInfo,
        //mut define_new: bool,
        constant: bool,
    ) -> Result<(Returns, Returns), RuntimeError> {
        info.pos = self.pos;
        
        let mut start_val = Returns::new();
        let mut inner_returns = Returns::new();

        //let mut defined = true;
        if let Some(UnaryOperator::Let) = self.operator {
            let val = self.define(&mut context, globals, &info)?;
            start_val = smallvec![(val, context)];
            return Ok((start_val, inner_returns));
        }

        use ast::IDClass;

        

        match &self.value.body {
            ast::ValueBody::Resolved(r) => start_val.push((*r, context.clone())),
            ast::ValueBody::SelfVal => {
                if let Some(val) = context.variables.get("self") {
                    start_val.push((*val, context.clone()))
                } else {
                    return Err(RuntimeError::RuntimeError {
                        message: "Cannot use \"self\" outside of macros!".to_string(),
                        info,
                    });
                }
            }
            ast::ValueBody::ID(id) => start_val.push((
                store_const_value(
                    match id.class_name {
                        IDClass::Group => {
                            if id.unspecified {
                                Value::Group(Group::next_free(&mut globals.closed_groups))
                            } else {
                                Value::Group(Group::new(id.number))
                            }
                        }
                        IDClass::Color => {
                            if id.unspecified {
                                Value::Color(Color::next_free(&mut globals.closed_colors))
                            } else {
                                Value::Color(Color::new(id.number))
                            }
                        }
                        IDClass::Block => {
                            if id.unspecified {
                                Value::Block(Block::next_free(&mut globals.closed_blocks))
                            } else {
                                Value::Block(Block::new(id.number))
                            }
                        }
                        IDClass::Item => {
                            if id.unspecified {
                                Value::Item(Item::next_free(&mut globals.closed_items))
                            } else {
                                Value::Item(Item::new(id.number))
                            }
                        }
                    },
                    1,
                    globals,
                    &context,
                ),
                context.clone(),
            )),
            ast::ValueBody::Number(num) => start_val.push((
                store_const_value(Value::Number(*num), 1, globals, &context),
                context.clone(),
            )),
            ast::ValueBody::Dictionary(dict) => {
                let new_info = info.clone();
                let (new_out, new_inner_returns) =
                    eval_dict(dict.clone(), &context, globals, new_info, constant)?;
                start_val = new_out;
                inner_returns = new_inner_returns;
            }
            ast::ValueBody::CmpStmt(cmp_stmt) => {
                let (evaled, returns) = cmp_stmt.to_scope(&context, globals, info.clone(), None)?;
                inner_returns.extend(returns);
                start_val.push((
                    store_const_value(Value::TriggerFunc(evaled), 1, globals, &context),
                    context.clone(),
                ));
            }

            ast::ValueBody::Expression(expr) => {
                let (evaled, returns) = expr.eval(&context, globals, info.clone(), constant)?;
                inner_returns.extend(returns);
                start_val.extend(evaled.iter().cloned());
            }

            ast::ValueBody::Bool(b) => start_val.push((
                store_const_value(Value::Bool(*b), 1, globals, &context),
                context.clone(),
            )),
            ast::ValueBody::Symbol(string) => {
                if string == "$" {
                    start_val.push((0, context.clone()));
                } else {
                    match context.variables.get(string) {
                        Some(value) => start_val.push((*value, context.clone())),
                        None => {
                            return Err(RuntimeError::UndefinedErr {
                                undefined: string.clone(),
                                info,
                                desc: "variable".to_string(),
                            });
                        }
                    }
                }
            }
            ast::ValueBody::Str(s) => start_val.push((
                store_const_value(Value::Str(s.clone()), 1, globals, &context),
                context.clone(),
            )),
            ast::ValueBody::Array(a) => {
                let new_info = info.clone();
                let (evaled, returns) =
                    all_combinations(a.clone(), &context, globals, new_info, constant)?;
                inner_returns.extend(returns);
                start_val = evaled
                    .iter()
                    .map(|x| {
                        (
                            store_value(Value::Array(x.0.clone()), 1, globals, &context),
                            x.1.clone(),
                        )
                    })
                    .collect();
            }
            ast::ValueBody::Import(i, f) => {
                //let mut new_contexts = context.clone();
                start_val = import_module(i, &context, globals, info.clone(), *f)?;
            }

            ast::ValueBody::TypeIndicator(name) => {
                start_val.push((
                    match globals.type_ids.get(name) {
                        Some(id) => {
                            store_const_value(Value::TypeIndicator(id.0), 1, globals, &context)
                        }
                        None => {
                            return Err(RuntimeError::UndefinedErr {
                                undefined: name.clone(),
                                info,
                                desc: "type".to_string(),
                            });
                        }
                    },
                    context.clone(),
                ));
            }

            ast::ValueBody::Ternary(t) => {
                
                let (evaled, returns) = t.conditional.eval(&context, globals, info.clone(), constant)?;
                // contexts of the conditional

                inner_returns.extend(returns);

                for (condition, context) in evaled { // through every condictional context
                    match &globals.stored_values[condition] {
                        Value::Bool(b) => {
                            let answer = if *b {&t.do_if} else {&t.do_else};

                            let (evaled, returns) = answer.eval(&context, globals, info.clone(), constant)?;
                            inner_returns.extend(returns);
                            start_val.extend(evaled);
                        }
                        a => {
                            return Err(RuntimeError::RuntimeError {
                                message: format!("Expected boolean condition in ternary statement, found {}", a.to_str(globals)),
                                info,

                            })
                        }
                    }
                }
            }

            ast::ValueBody::Switch(expr, cases) => {
                // ok so in spwn you have to always assume every expression will split the context, that is,
                // output multiple values in multiple contexts. This is called context splitting. A list of 
                // values and contexts (Vec<(Value, Context)>) is called bundled together in a type called Returns
                let (evaled, returns) = expr.eval(&context, globals, info.clone(), constant)?;
                //inner returns are return statements that are inside the expession, for example in a function/trigger context/ whatever we call it now
                inner_returns.extend(returns);

                // now we loop through every value the first expression outputted
                for (val1, context) in evaled {
                    //lets store the current contexts we are working with in a vector, starting with only the context
                    // outputted from the first expression
                    let mut contexts = vec![context.clone()];


                    for case in cases {
                        // if there are no contexts left to deal with, we can leave the loop
                        if contexts.is_empty() {
                            break
                        }

                        match &case.typ {
                            ast::CaseType::Value(v) => {
                                // in this type of case we want to check if the original expression is
                                // equal to some value. for this, we use the == operator

                                // lets first evaluate the value we will compare to
                                // remember, we have to evaluate it in all the contexts we are working with
                                let mut all_values = Vec::new();
                                for c in &contexts {
                                    let (evaled, returns) = v.eval(c, globals, info.clone(), constant)?;
                                    inner_returns.extend(returns);
                                    all_values.extend(evaled);
                                }

                                // lets clear the contexts list for now, as we will refill it
                                // with new contexts from the next few evaluations
                                contexts.clear();
                                
                                // looping through all the values of the expression we just evaled
                                for (val2, c) in all_values {

                                    // lets compare the two values with the == operator
                                    // since this is an expression in itself, we also have to assume
                                    // this will output multiple values
                                    let results = handle_operator(
                                        val1, 
                                        val2, 
                                        "_equal_", 
                                        &c, 
                                        globals, 
                                        &info
                                    )?;

                                    // lets loop through all those result values
                                    for (r, c) in results {
                                        if let Value::Bool(b) = globals.stored_values[r] {
                                            if b {
                                                // if the two values match, we output this value to the output "start val"
                                                // we can't break here, because the two values might only match in this one context,
                                                // and there may be more contexts left to check
                                                let (evaled, returns) = case.body.eval(&c, globals, info.clone(), constant)?;
                                                inner_returns.extend(returns);
                                                start_val.extend(evaled);
                                            } else {
                                                // if they dont match, we keep going through the cases in this context
                                                contexts.push(c)
                                            }
                                        } else {
                                            // if the == operator for that type doesn't output a boolean, it can't be
                                            // used in a switch statement
                                            return Err(RuntimeError::RuntimeError {
                                                message: "== operator returned non-boolean value".to_string(),
                                                info,
                                                
                                            });
                                        }
                                    }
                                }

                            }
                            ast::CaseType::Pattern(p) => {
                                // this is pretty much the same as the one before, except that we use .matches_pat
                                // to check instead of ==
                                let mut all_patterns = Vec::new();
                                for c in &contexts {
                                    let (evaled, returns) = p.eval(c, globals, info.clone(), constant)?;
                                    inner_returns.extend(returns);
                                    all_patterns.extend(evaled);
                                }
                                contexts.clear();
                                
                                for (pat, c) in all_patterns {
                                    let pat_val = globals.stored_values[pat].clone();
                                    let b = globals.stored_values[val1].clone().matches_pat(&pat_val, &info, globals, &context)?;

                                    if b {
                                        let (evaled, returns) = case.body.eval(&c, globals, info.clone(), constant)?;
                                        inner_returns.extend(returns);
                                        start_val.extend(evaled);
                                    } else {
                                        contexts.push(c)
                                    }
                                        
                                }
                            }

                            ast::CaseType::Default => {
                                //this should be the last case, so we just return the body
                                for c in &contexts {
                                    let (evaled, returns) = case.body.eval(&c, globals, info.clone(), constant)?;
                                    inner_returns.extend(returns);
                                    start_val.extend(evaled);
                                }
                            }
                        }
                        
                    }
                }



            }
            ast::ValueBody::Obj(o) => { // parsing an obj

                let mut all_expr: Vec<ast::Expression> = Vec::new(); // all expressions

                for prop in &o.props { // iterate through obj properties

                    all_expr.push(prop.0.clone()); // this is the object key expression
                    all_expr.push(prop.1.clone()); // this is the object value expression
                }
                let new_info = info.clone();

                let (evaled, returns) =
                    all_combinations(all_expr, &context, globals, new_info, constant)?; // evaluate all expressions gathered
                inner_returns.extend(returns);
                for (expressions, context) in evaled {
                    let mut obj: Vec<(u16, ObjParam)> = Vec::new();
                    for i in 0..(o.props.len()) {

                        let o_key = expressions[i * 2]; 
                        let o_val = expressions[i * 2 + 1];
                        // hopefully self explanatory

                        let (key, pattern) = match &globals.stored_values[o_key] {
                        // key = int of the id, pattern = what type should be expected from the value

                            Value::Number(n) => { // number, i have no clue why people would use this over an obj_key
                                let out = *n as u16;

                                if o.mode == ast::ObjectMode::Trigger && (out == 57 || out == 62) {
                                    return Err(RuntimeError::RuntimeError {
                                        message: "You are not allowed to set the group ID(s) or the spawn triggered state of a @trigger. Use obj instead".to_string(),
                                        info,
                                    })
                                }

                                (out, None)
                            },
                            Value::Dict(d) => { // this is specifically for object_key dicts
                                let gotten_type = d.get(TYPE_MEMBER_NAME);
                                if gotten_type == None ||  globals.stored_values[*gotten_type.unwrap()] != Value::TypeIndicator(19) { // 19 = object_key??
                                    return Err(RuntimeError::RuntimeError {
                                        message: "expected either @number or @object_key as object key".to_string(),
                                        info,
                                    })
                                }
                                let id = d.get("id");
                                if id == None {
                                    return Err(RuntimeError::RuntimeError { // object_key has an ID member for the key basically
                                        message: "object key has no 'id' member".to_string(),
                                        info,
                                    })
                                }
                                let pattern = d.get("pattern");
                                if pattern == None {
                                    return Err(RuntimeError::RuntimeError { // same with pattern, for the expected type
                                        message: "object key has no 'pattern' member".to_string(),
                                        info,
                                    })
                                }

                                (match &globals.stored_values[*id.unwrap()] { // check if the ID is actually an int. it should be
                                    Value::Number(n) => {
                                        let out = *n as u16;

                                        if o.mode == ast::ObjectMode::Trigger && (out == 57 || out == 62) { // group ids and stuff on triggers
                                            return Err(RuntimeError::RuntimeError {
                                                message: "You are not allowed to set the group ID(s) or the spawn triggered state of a @trigger. Use obj instead".to_string(),
                                                info,
                                            })
                                        }
                                        out
                                    }
                                    _ => return Err(RuntimeError::RuntimeError {
                                        message: format!("object key's id has to be @number, found {}", globals.get_type_str(*id.unwrap())),
                                        info,
                                    })
                                }, Some(globals.stored_values[*pattern.unwrap()].clone()))
                                
                            }
                            a => {
                                return Err(RuntimeError::RuntimeError {
                                    message: format!(
                                        "expected either @number or @object_key as object key, found: {}",
                                        a.to_str(globals)
                                    ),
                                    info,
                                })
                            }
                        };

                        obj.push((
                            key,
                            {   // parse the value
                                let val = globals.stored_values[o_val].clone();

                                if let Some(pat) = pattern { // check if pattern is actually enforced (not null)
                                    if !val.matches_pat(&pat, &info, globals, &context)? {
                                        return Err(RuntimeError::RuntimeError {
                                            message: format!(
                                                "key required value to match {}, found {}",
                                                pat.to_str(globals), val.to_str(globals)
                                            ),
                                            info,
                                        })
                                    }
                                }
                                let err = Err(RuntimeError::RuntimeError {
                                    message: format!(
                                        "{} is not a valid object value",
                                        val.to_str(globals)
                                    ),
                                    info: info.clone(),
                                });
                                
                                match &val { // its just converting value to objparam basic level stuff
                                    Value::Number(n) => {
                                        
                                        ObjParam::Number(*n)
                                    },
                                    Value::Str(s) => ObjParam::Text(s.clone()),
                                    Value::TriggerFunc(g) => ObjParam::Group(g.start_group),

                                    Value::Group(g) => ObjParam::Group(*g),
                                    Value::Color(c) => ObjParam::Color(*c),
                                    Value::Block(b) => ObjParam::Block(*b),
                                    Value::Item(i) => ObjParam::Item(*i),

                                    Value::Bool(b) => ObjParam::Bool(*b),

                                    Value::Array(a) => ObjParam::GroupList({
                                        let mut out = Vec::new();
                                        for s in a {
                                            out.push(match globals.stored_values[*s] {
                                                Value::Group(g) => g,
                                                _ => return Err(RuntimeError::RuntimeError {
                                                    message: "Arrays in object parameters can only contain groups".to_string(),
                                                    info,
                                                })
                                            })
                                        }
                                        
                                        out
                                    }),
                                    Value::Dict(d) => {
                                        if let Some(t) = d.get(TYPE_MEMBER_NAME) {
                                            if let Value::TypeIndicator(t) = globals.stored_values[*t] {
                                                if t == 20 { // type indicator number 20 is epsilon ig
                                                    ObjParam::Epsilon
                                                } else {
                                                    return err;
                                                }
                                            } else {
                                                return err;
                                            }
                                        } else {
                                            return err;
                                        }
                                    }
                                    _ => {
                                        return err;
                                    }
                                }
                        
                            },
                        ))
                    }
                    
                    start_val.push((
                        store_const_value(Value::Obj(obj, o.mode), 1, globals, &context),
                        context,
                    ));
                }
            }

            ast::ValueBody::Macro(m) => {
                let mut all_expr: Vec<ast::Expression> = Vec::new();
                for arg in &m.args {
                    if let Some(e) = &arg.1 {
                        all_expr.push(e.clone());
                    }

                    if let Some(e) = &arg.3 {
                        all_expr.push(e.clone());
                    }
                }
                let new_info = info.clone();
                let (argument_possibilities, returns) =
                    all_combinations(all_expr, &context, globals, new_info, constant)?;
                inner_returns.extend(returns);
                for defaults in argument_possibilities {
                    let mut args: Vec<(String, Option<StoredValue>, ast::Tag, Option<StoredValue>)> =
                        Vec::new();
                    let mut expr_index = 0;
                    
                    for arg in m.args.iter() {
                        let def_val = match &arg.1 {
                            Some(_) => {
                                expr_index += 1;
                                Some(
                                    clone_value(defaults.0[expr_index - 1], 1, globals, defaults.1.start_group, true)
                                )
                            }
                            None => None,
                        };
                        let pat = match &arg.3 {
                            Some(_) => {
                                expr_index += 1;
                                Some(defaults.0[expr_index - 1])
                            }
                            None => None,
                        };
                        args.push((
                            arg.0.clone(),
                            def_val,
                            arg.2.clone(),
                            pat,
                        ));
                    }

                    start_val.push((
                        store_const_value(
                            Value::Macro(Box::new(Macro {
                                args,
                                body: m.body.statements.clone(),
                                def_context: defaults.1.clone(),
                                def_file: info.current_file.clone(),
                                tag: m.properties.clone(),
                            })),
                            1,
                            globals,
                            &context,
                        ),
                        defaults.1,
                    ))
                }
            }
            //ast::ValueLiteral::Resolved(r) => out.push((r.clone(), context)),
            ast::ValueBody::Null => start_val.push((1, context.clone())),
        };

        let mut path_iter = self.path.iter();
        let mut with_parent: Vec<(StoredValue, Context, StoredValue)> =
            start_val.iter().map(|x| (x.0, x.1.clone(), 1)).collect();
        for p in &mut path_iter {
            // if !defined {
            //     use crate::fmt::SpwnFmt;
            //     return Err(RuntimeError::RuntimeError {
            //         message: format!("Cannot run {} on an undefined value", p.fmt(0)),
            //         info,
            //     });
            // }
            match &p {
                ast::Path::Member(m) => {
                    for x in &mut with_parent {
                        let val = globals.stored_values[x.0].clone(); // this is the object we are getting member of
                        *x = ( 
                            match val.member(m.clone(), &x.1, globals) {
                                Some(m) => m,
                                None => {
                                    return Err(RuntimeError::UndefinedErr {
                                        undefined: m.clone(),
                                        info,
                                        desc: "member".to_string(),
                                    });
                                }
                            },
                            x.1.clone(),
                            x.0,
                        )
                    }
                }

                ast::Path::Associated(a) => {
                    for x in &mut with_parent {
                        *x = (
                            match &globals.stored_values[x.0] {
                                Value::TypeIndicator(t) => match globals.implementations.get(&t) {
                                    Some(imp) => match imp.get(a) {
                                        Some((val, _)) => {
                                            if let Value::Macro(m) = &globals.stored_values[*val] {
                                                if !m.args.is_empty() && m.args[0].0 == "self" {
                                                    return Err(RuntimeError::RuntimeError {
                                                        message: "Cannot access method (macro with a \"self\" argument) using \"::\"".to_string(),
                                                        info,
                                                    });
                                                }
                                            }
                                            *val
                                        }
                                        None => {
                                            let type_name =
                                                find_key_for_value(&globals.type_ids, *t).unwrap();
                                            return Err(RuntimeError::RuntimeError {
                                                message: format!(
                                                    "No {} property on type @{}",
                                                    a, type_name
                                                ),
                                                info,
                                            });
                                        }
                                    },
                                    None => {
                                        let type_name =
                                            find_key_for_value(&globals.type_ids, *t).unwrap();
                                        return Err(RuntimeError::RuntimeError {
                                            message: format!(
                                                "No values are implemented on @{}",
                                                type_name
                                            ),
                                            info,
                                        });
                                    }
                                },
                                a => {
                                    return Err(RuntimeError::RuntimeError {
                                        message: format!(
                                            "Expected type indicator, found: {}",
                                            a.to_str(globals)
                                        ),
                                        info,
                                    })
                                }
                            },
                            x.1.clone(),
                            x.0,
                        )
                    }
                }

                ast::Path::Index(i) => {
                    let mut new_out: Vec<(StoredValue, Context, StoredValue)> = Vec::new();

                    for (prev_v, prev_c, _) in with_parent.clone() {
                        
                        match globals.stored_values[prev_v].clone() {
                            Value::Array(arr)  => {
                                
                                let (evaled, returns) =
                                    i.eval(&prev_c, globals, info.clone(), constant)?;
                                inner_returns.extend(returns);
                                for index in evaled {
                                    match &globals.stored_values[index.0] {
                                        Value::Number(n) => {
                                            let len = arr.len();
                                            if (*n) < 0.0 && (-*n) as usize >= len {
                                                return Err(RuntimeError::RuntimeError {
                                                    message: format!("Index too low! Index is {}, but length is {}.", n, len),
                                                    info,
                                                });
                                            }
                                            
                                            if *n as usize >= len {
                                                return Err(RuntimeError::RuntimeError {
                                                    message: format!("Index too high! Index is {}, but length is {}.", n, len),
                                                    info,
                                                });
                                            }

                                            if *n < 0.0 {
                                                new_out.push((arr[len - (-n as usize)], index.1, prev_v));
                                            } else {
                                                new_out.push((arr[*n as usize], index.1, prev_v));
                                            }

                                            
                                        }
                                        _ => {
                                            return Err(RuntimeError::RuntimeError {
                                                message: format!(
                                                    "expected @number in index, found @{}",
                                                    globals.get_type_str(index.0)
                                                ),
                                                info,
                                            })
                                        }
                                    }
                                }
                            }
                            Value::Dict(d)  => {
                                
                                let (evaled, returns) =
                                    i.eval(&prev_c, globals, info.clone(), constant)?;
                                inner_returns.extend(returns);
                                for index in evaled {
                                    match &globals.stored_values[index.0] {
                                        Value::Str(s) => {
                                            if !d.contains_key(s) {
                                                return Err(RuntimeError::RuntimeError {
                                                    message: format!("Cannot find key '{}' in dictionary",s),
                                                    info,
                                                })
                                            }
                                            new_out.push((d[s], index.1, prev_v));  
                                        }
                                        _ => {
                                            return Err(RuntimeError::RuntimeError {
                                                message: format!(
                                                    "expected @string in index, found @{}",
                                                    globals.get_type_str(index.0)
                                                ),
                                                info,
                                            })
                                        }
                                    }
                                }
                            }

                            Value::Obj(o, _) => {

                                let (evaled, returns) =
                                    i.eval(&prev_c, globals, info.clone(), constant)?;
                                inner_returns.extend(returns);
                                for index in evaled {
                                    match &globals.stored_values[index.0] {
                                        Value::Dict(d) => {
                                            let gotten_type = d.get(TYPE_MEMBER_NAME);
                                            if gotten_type == None ||  globals.stored_values[*gotten_type.unwrap()] != Value::TypeIndicator(19) { // 19 = object_key??
                                                return Err(RuntimeError::RuntimeError {
                                                    message: "expected either @number or @object_key in index".to_string(),
                                                    info,
                                                })
                                            }

                                            let id = d.get("id");
                                            if id == None {
                                                return Err(RuntimeError::RuntimeError { // object_key has an ID member for the key basically
                                                    message: "object key has no 'id' member".to_string(),
                                                    info,
                                                })
                                            }
                                            let okey = match &globals.stored_values[*id.unwrap()] { // check if the ID is actually an int. it should be
                                                Value::Number(n) => {
                                                    *n as u16
                                                }
                                                _ => return Err(RuntimeError::RuntimeError {
                                                    message: format!("object key's id has to be @number, found {}", globals.get_type_str(*id.unwrap())),
                                                    info,
                                                })
                                            };

                                            let mut contains = false;
                                            for iter in o.iter() {
                                                if iter.0 == okey {
                                                    contains = true;

                                                    let out_val = match &iter.1 { // its just converting value to objparam basic level stuff
                                                        ObjParam::Number(n) => Value::Number(*n),
                                                        ObjParam::Text(s) => Value::Str(s.clone()),

                                                        ObjParam::Group(g) => Value::Group(*g),
                                                        ObjParam::Color(c) => Value::Color(*c),
                                                        ObjParam::Block(b) => Value::Block(*b),
                                                        ObjParam::Item(i) => Value::Item(*i),

                                                        ObjParam::Bool(b) => Value::Bool(*b),

                                                        ObjParam::GroupList(g) => {
                                                            let mut out = Vec::new();
                                                            for s in g {
                                                                let stored = store_const_value(Value::Group(*s), 1, globals, &index.1);
                                                                out.push(stored);
                                                            }
                                                            Value::Array(out)
                                                        },
                                                        
                                                        ObjParam::Epsilon => {
                                                            let mut map = HashMap::<String, StoredValue>::new();
                                                            let stored = store_const_value(Value::TypeIndicator(20), 1, globals, &index.1);
                                                            map.insert(TYPE_MEMBER_NAME.to_string(), stored);
                                                            Value::Dict(map)
                                                        }
                                                    };
                                                    let stored = store_const_value(out_val, globals.stored_values.map.get(&prev_v).unwrap().lifetime, globals, &index.1);
                                                    new_out.push((stored, index.1, prev_v));
                                                    break;
                                                }
                                            }

                                            if !contains {
                                                return Err(RuntimeError::RuntimeError {
                                                    message: "Cannot find key in object".to_string(),
                                                    info,
                                                });
                                            }

                                        }
                                        _ => {
                                            return Err(RuntimeError::RuntimeError {
                                                message: format!(
                                                    "expected @object_key or @number in index, found @{}",
                                                    globals.get_type_str(index.0)
                                                ),
                                                info,
                                            })
                                        }
                                    }
                                }

                            }
                            Value::Str(s)  => {
                                let arr: Vec<char> = s.chars().collect();
                                
                                let (evaled, returns) =
                                    i.eval(&prev_c, globals, info.clone(), constant)?;
                                inner_returns.extend(returns);
                                for index in evaled {
                                    match &globals.stored_values[index.0] {
                                        Value::Number(n) => {
                                            let len = arr.len();
                                            if (*n) < 0.0 && (-*n) as usize >= len {
                                                return Err(RuntimeError::RuntimeError {
                                                    message: format!("Index too low! Index is {}, but length is {}.", n, len),
                                                    info,
                                                });
                                            }
                                            
                                            if *n as usize >= len {
                                                return Err(RuntimeError::RuntimeError {
                                                    message: format!("Index too high! Index is {}, but length is {}.", n, len),
                                                    info,
                                                });
                                            }

                                            let val = if *n < 0.0 {
                                                Value::Str(arr[len - (-n as usize)].to_string())
                                               
                                            } else {
                                                Value::Str(arr[*n as usize].to_string())
                                            };
                                            let stored = store_const_value(val, 1, globals, &index.1);

                                            new_out.push((stored, index.1, prev_v));
                                            
                                        }
                                        _ => {
                                            return Err(RuntimeError::RuntimeError {
                                                message: format!(
                                                    "expected @number in index, found @{}",
                                                    globals.get_type_str(index.0)
                                                ),
                                                info,
                                            })
                                        }
                                    }
                                }
                            }
                            a => {
                                return Err(RuntimeError::RuntimeError {
                                    message: format!(
                                        "Cannot index this type: {}",
                                        a.to_str(globals)
                                    ),
                                    info,
                                })
                            }
                        }
                    }

                    with_parent = new_out
                }

                ast::Path::Increment => {
                    for (prev_v,prev_c, _) in &mut with_parent {
                        let is_mutable = globals.stored_values.map[&prev_v].mutable;
                        match &mut globals.stored_values[*prev_v] {
                            Value::Number(n) => {
                                *n += 1.0;
                                *prev_v = store_val_m(Value::Number(*n - 1.0),1, globals, prev_c, is_mutable);
                            }
                            _ => {
                                return Err(RuntimeError::RuntimeError {
                                    message: "Cannot increment this type".to_string(),
                                    info,
                                })
                            }
                        }
                    } 
                }

                ast::Path::Decrement => {
                    for (prev_v,prev_c, _) in &mut with_parent {
                        let is_mutable = globals.stored_values.map[&prev_v].mutable;
                        match &mut globals.stored_values[*prev_v] {
                            Value::Number(n) => {
                                *n -= 1.0;                          
                                *prev_v = store_val_m(Value::Number(*n + 1.0),1, globals, prev_c, is_mutable);
                            }
                            _ => {
                                return Err(RuntimeError::RuntimeError {
                                    message: "Cannot decrement this type".to_string(),
                                    info,
                                })
                            }
                        }
                    } 
                }

                ast::Path::Constructor(defs) => {
                    let mut new_out: Vec<(StoredValue, Context, StoredValue)> = Vec::new();

                    for (prev_v, prev_c, _) in &with_parent {
                        match globals.stored_values[*prev_v].clone() {
                            Value::TypeIndicator(t) => {
                                let (dicts, returns) = ast::ValueBody::Dictionary(defs.clone())
                                    .to_variable()
                                    .to_value(prev_c.clone(), globals, info.clone(), constant)?;
                                inner_returns.extend(returns);
                                for dict in &dicts {
                                    let stored_type =
                                        store_value(Value::TypeIndicator(t), 1, globals, &context);
                                    if let Value::Dict(map) = &mut globals.stored_values[dict.0] {
                                        (*map).insert(TYPE_MEMBER_NAME.to_string(), stored_type);
                                    } else {
                                        unreachable!()
                                    }

                                    new_out.push((dict.0, dict.1.clone(), *prev_v));
                                }
                            }
                            a => {
                                return Err(RuntimeError::RuntimeError {
                                message: format!(
                                    "Attempted to construct on a value that is not a type indicator: {}",
                                    a.to_str(globals)
                                ),
                                info,
                            });
                            }
                        }
                    }
                    with_parent = new_out
                }

                ast::Path::Call(args) => {
                    for (v, cont, parent) in with_parent.clone().iter() {
                        match globals.stored_values[*v].clone() {
                            Value::Macro(m) => {
                                let (evaled, returns) = execute_macro(
                                    (*m, args.clone()),
                                    cont,
                                    globals,
                                    *parent,
                                    info.clone(),
                                )?;
                                inner_returns.extend(returns);
                                with_parent =
                                    evaled.iter().map(|x| (x.0, x.1.clone(), *v)).collect();
                            }

                            Value::TypeIndicator(_) => {
                                if args.len() != 1 { // cast takes 1 argument only
                                    return Err(RuntimeError::RuntimeError {
                                        message: format!("casting takes one argument, but {} were provided", args.len()),
                                        info,
                                    })
                                }

                                // one value for each context
                                let mut all_values = Returns::new();

                                //find out whats in the thing we are casting first, its a tuple because contexts and stuff
                                let (evaled, returns) = args[0].value.eval(cont, globals, info.clone(), constant)?; 

                                //return statements are weird in spwn 
                                inner_returns.extend(returns);

                                // go through each context, c = context
                                for (val, c) in evaled {
                                    let evaled = handle_operator(val, *v, "_as_", &c, globals, &info)?; // just use the "as" operator
                                    all_values.extend(evaled);
                                }
                                
                                with_parent =
                                all_values.iter().map(|x| (x.0, x.1.clone(), *v)).collect(); // not sure but it looks important
                            }

                            Value::BuiltinFunction(name) => {
                                let (evaled_args, returns) = all_combinations(
                                    args.iter().map(|x| x.value.clone()).collect(),
                                    cont,
                                    globals,
                                    info.clone(),
                                    constant,
                                )?;
                                inner_returns.extend(returns);

                                let mut all_values = Returns::new();

                                for (args, context) in evaled_args {
                                    let evaled = built_in_function(
                                        &name,
                                        args,
                                        info.clone(),
                                        globals,
                                        &context,
                                    )?;
                                    all_values
                                        .push((store_value(evaled, 1, globals, &context), context))
                                }

                                with_parent =
                                    all_values.iter().map(|x| (x.0, x.1.clone(), *v)).collect();
                            }
                            a => {
                                return Err(RuntimeError::RuntimeError {
                                    message: format!(
                                        "Cannot call ( ... ) on '{}'",
                                        a.to_str(globals)
                                    ),
                                    info,
                                })
                            }
                        }
                    }
                }
            };
        }

        let mut out: Returns = with_parent.iter().map(|x| (x.0, x.1.clone())).collect();

        use ast::UnaryOperator;
        if let Some(o) = &self.operator {
            for final_value in &mut out {
                match o {
                    UnaryOperator::Minus => {
                        if let Value::Number(n) = globals.stored_values[final_value.0] {
                            *final_value = (
                                store_value(Value::Number(-n), 1, globals, &context),
                                final_value.1.clone(),
                            );
                        } else {
                            return Err(RuntimeError::RuntimeError {
                                message: "Cannot make non-number type negative".to_string(),
                                info,
                            });
                        }
                    }

                    UnaryOperator::Increment => {
                        if let Value::Number(n) = &mut globals.stored_values[final_value.0] {
                            *n += 1.0;
                        } else {
                            return Err(RuntimeError::RuntimeError {
                                message: "Cannot increment non-number type".to_string(),
                                info,
                            });
                        }
                    }

                    UnaryOperator::Decrement => {
                        if let Value::Number(n) = &mut globals.stored_values[final_value.0] {
                            *n -= 1.0;
                        } else {
                            return Err(RuntimeError::RuntimeError {
                                message: "Cannot decrement non-number type".to_string(),
                                info,
                            });
                        }
                    }

                    UnaryOperator::Not => {
                        if let Value::Bool(b) = globals.stored_values[final_value.0] {
                            *final_value = (
                                store_value(Value::Bool(!b), 1, globals, &context),
                                final_value.1.clone(),
                            );
                        } else {
                            return Err(RuntimeError::RuntimeError {
                                message: "Cannot negate non-boolean type".to_string(),
                                info,
                            });
                        }
                    }

                    UnaryOperator::Let => (),

                    UnaryOperator::Range => {
                        if let Value::Number(n) = globals.stored_values[final_value.0] {
                            let end = convert_to_int(n, &info)?;
                            *final_value = (
                                store_value(
                                    Value::Range(0, end, 1),
                                    1,
                                    globals,
                                    &context,
                                ),
                                final_value.1.clone(),
                            );
                        } else {
                            return Err(RuntimeError::RuntimeError {
                                message: "Expected number in range".to_string(),
                                info,
                            });
                        }
                    }
                }
            }
        }

        // if self
        //         .tag
        //         .tags
        //         .iter()
        //         .any(|x| x.0 == "allow_context_change")
        // {
            
        //     for (val, _) in &out {
        //         (*globals
        //             .stored_values
        //             .map
        //             .get_mut(val)
        //             .expect("index not found"))
        //             .allow_context_change = true;

                
        //     }
        // }

        Ok((out, inner_returns))
    }

    //more like is_undefinable
    pub fn is_undefinable(&self, context: &Context, globals: &mut Globals) -> bool {
        //use crate::fmt::SpwnFmt;
        // if self.operator == Some(ast::UnaryOperator::Let) {
        //     return true
        // }
        
        // println!("hello? {}", self.fmt(0));
        let mut current_ptr = match &self.value.body {
            ast::ValueBody::Symbol(a) => {
                if let Some(ptr) = context.variables.get(a) {
                    *ptr
                } else {
                    return false;
                }
            }

            ast::ValueBody::TypeIndicator(t) => {
                if let Some(typ) = globals.type_ids.get(t) {
                    store_const_value(Value::TypeIndicator(typ.0), 1, globals, context)
                } else {
                    return false;
                }
            }

            ast::ValueBody::SelfVal => {
                if let Some(ptr) = context.variables.get("self") {
                    *ptr
                } else {
                    return false;
                }
            }

            _ => return true,
        };

        for p in &self.path {
            match p {
                ast::Path::Member(m) => {
                    
                    if let Value::Dict(d) = &globals.stored_values[current_ptr] {
                        match d.get(m) {
                            Some(s) => current_ptr = *s,
                            None => return false,
                        }
                        
                    } else {
                        
                        return true;
                    }
                }
                ast::Path::Associated(m) => match &globals.stored_values[current_ptr] {
                    Value::TypeIndicator(t) => match globals.implementations.get(t) {
                        Some(imp) => {
                            if let Some(val) = imp.get(m) {
                                current_ptr = val.0;
                            } else {
                                return false;
                            }
                        }
                        None => return false,
                    },
                    _ => {
                        return true;
                    }
                },
                ast::Path::Index(i) => {
                    if i.values.len() == 1 {
                        if let ast::ValueBody::Str(s) = &i.values[0].value.body {
                            match &globals.stored_values[current_ptr] {
                                Value::Dict(d)  => {
                                    if let Some(_) = d.get(s) {
                                        return true
                                    } else {
                                        return false
                                    }
                                    
                                }
                                _ => return true,
                            }
                        } else {
                            return true
                        }
                    } else {
                        return true
                    }
                    
                }
                _ => return true,
            }
        }

        true
    }
    
    pub fn define(
        &self,
        //value: StoredValue,
        context: &mut Context,
        globals: &mut Globals,
        info: &CompilerInfo,
    ) -> Result<StoredValue, RuntimeError> {
        // when None, the value is already defined
        use crate::fmt::SpwnFmt;
        let mut defined = true;
        

        let value = match &self.operator {
            Some(ast::UnaryOperator::Let) => store_value(Value::Null, 1, globals, context),
            None => store_const_value(Value::Null, 1, globals, context),
            a => {
                return Err(RuntimeError::RuntimeError {
                    message: format!("Cannot use operator {:?} in definition", a),
                    info: info.clone(),
                })
            }
        };

        let mut current_ptr = match &self.value.body {
            ast::ValueBody::Symbol(a) => {
                if let Some(ptr) = context.variables.get(a) {
                    *ptr
                } else {
                    (*context).variables.insert(a.clone(), value);
                    defined = false;
                    value
                }
            }

            ast::ValueBody::TypeIndicator(t) => {
                if let Some(typ) = globals.type_ids.get(t) {
                    store_const_value(Value::TypeIndicator(typ.0), 1, globals, context)
                } else {
                    return Err(RuntimeError::RuntimeError {
                        message: format!("Use a type statement to define a new type: type {}", t),
                        info: info.clone(),
                    });
                }
            }

            ast::ValueBody::SelfVal => {
                if let Some(ptr) = context.variables.get("self") {
                    *ptr
                } else {
                    return Err(RuntimeError::RuntimeError {
                        message: format!("Cannot use 'self' outside macros"),
                        info: info.clone(),
                    });
                }
            }

            a => {
                return Err(RuntimeError::RuntimeError {
                    message: format!("Expected symbol or type-indicator, found {}", a.fmt(0)),
                    info: info.clone(),
                })
            }
        };

        

        for p in &self.path {
            (*globals.stored_values.map.get_mut(&value).unwrap()).lifetime = globals.get_lifetime(current_ptr);
            if !defined {
                return Err(RuntimeError::RuntimeError {
                    message: format!("Cannot run {} on an undefined value", p.fmt(0)),
                    info: info.clone(),
                });
            }

            match p {
                ast::Path::Member(m) => {
                    let val = globals.stored_values[current_ptr].clone();
                    match val.member(m.clone(), &context, globals) {
                        Some(s) => current_ptr = s,
                        None => {
                            let stored = globals.stored_values.map.get_mut(&current_ptr).unwrap();
                            if !stored.mutable {
                                return Err(RuntimeError::RuntimeError {
                                    message: "Cannot edit members of a constant value".to_string(),
                                    info: info.clone(),
                                });
                            }
                            if let Value::Dict(d) = &mut stored.val {
                                (*d).insert(m.clone(), value);
                                defined = false;
                                current_ptr = value;
                            } else {
                                return Err(RuntimeError::RuntimeError {
                                    message: "Cannot edit members of a non-dictionary value"
                                        .to_string(),
                                    info: info.clone(),
                                });
                            }
                        }
                    };
                }
                ast::Path::Index(i) => {
                    let (evaled, _) = i.eval(&context, globals, info.clone(), true)?;
                    let first_context_eval = evaled[0].0;
                    match &globals.stored_values[current_ptr] {
                        Value::Dict(d)  => {
                            if evaled.len() > 1 {
                                println!("Warning: context splitting inside of an index definition. Use $.dict_add for better results");
                            }
                            if let Value::Str(st) = globals.stored_values[first_context_eval].clone() {

                                match d.get(&st) {
                                    Some(_) => current_ptr = first_context_eval,
                                    None => {
                                        let stored = globals.stored_values.map.get_mut(&current_ptr).unwrap();
                                        if !stored.mutable {
                                            return Err(RuntimeError::RuntimeError {
                                                message: "Cannot edit members of a constant value".to_string(),
                                                info: info.clone(),
                                            });
                                        }
                                        if let Value::Dict(d) = &mut stored.val {
                                            (*d).insert(st.to_string(), value);
                                            defined = false;
                                            current_ptr = value;
                                        } else {
                                            unreachable!();
                                        }
                                    }
                                };
                            } else {
                                return Err(RuntimeError::RuntimeError {
                                    message: "Only string indexes are supported for dicts".to_string(),
                                    info: info.clone(),
                                });
                            }
                        }
                        _ => {
                            return Err(RuntimeError::RuntimeError {
                                message: "Other values are not supported yet".to_string(),
                                info: info.clone()
                            })
                        },
                    }
                }
                ast::Path::Associated(m) => {
                    match &globals.stored_values[current_ptr] {
                        Value::TypeIndicator(t) => match (*globals).implementations.get_mut(t) {
                            Some(imp) => {
                                if let Some((val,_)) = imp.get(m) {
                                    current_ptr = *val;
                                } else {
                                    (*imp).insert(m.clone(), (value, true));
                                    defined = false;
                                    current_ptr = value;
                                }
                            }
                            None => {
                                let mut new_imp = HashMap::new();
                                new_imp.insert(m.clone(), (value, true));
                                (*globals).implementations.insert(*t, new_imp);
                                defined = false;
                                current_ptr = value;
                            }
                        },
                        a => {
                            return Err(RuntimeError::RuntimeError {
                                message: format!(
                                    "Expected a type-indicator to define an implementation on, found {}",
                                    a.to_str(globals)
                                ),
                                info: info.clone(),
                            });
                        }
                    };
                }
                _ => {
                    return Err(RuntimeError::RuntimeError {
                        message: format!("Cannot run {} in a definition expression", p.fmt(0)),
                        info: info.clone(),
                    })
                }
            }
        }
        
        if defined {
            Err(RuntimeError::RuntimeError {
                message: format!("{} is already defined!", self.fmt(0)),
                info: info.clone(),
            })
        } else {
            Ok(current_ptr)
        }
    }
}

impl ast::CompoundStatement {
    pub fn to_scope(
        &self,
        context: &Context,
        globals: &mut Globals,
        info: CompilerInfo,
        start_group: Option<Group>,
    ) -> Result<(TriggerFunction, Returns), RuntimeError> {
        //create the function context
        let mut new_context = context.next_fn_id(globals);

        //pick a start group
        let start_group = if let Some(g) = start_group {
            g
        } else {
            Group::next_free(&mut globals.closed_groups)
        };

        new_context.start_group = start_group;
        
        let new_info = info;
        let (contexts, inner_returns) =
            compile_scope(&self.statements, smallvec![new_context], globals, new_info)?;

        for c in contexts {
            if let Some((i, t)) = c.broken {
                match t {
                    BreakType::Loop => {
                        return Err(RuntimeError::RuntimeError {
                            message: "break statement is never used because it's inside a trigger function"
                                .to_string(),
                            info: i,
                        });
                    }

                    BreakType::ContinueLoop => {
                        return Err(RuntimeError::RuntimeError {
                            message: "continue statement is never used because it's inside a trigger function"
                                .to_string(),
                            info: i,
                        });
                    }

                    BreakType::Macro => {
                        return Err(RuntimeError::RuntimeError {
                            message: "return statement is never used because it's inside a trigger function (consider putting the return statement in an arrow statement)"
                                .to_string(),
                            info: i,
                        });
                    }
                }
                
            }
        }

        Ok((TriggerFunction { start_group }, inner_returns))
    }
}
