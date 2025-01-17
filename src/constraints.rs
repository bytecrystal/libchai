//! 优化问题的约束。

use crate::{
    config::{AtomicConstraint, MappedKey},
    error::Error,
    representation::{assemble, Element, Key, KeyMap, Representation},
};
use rand::{seq::SliceRandom, thread_rng, Rng};
use std::collections::{HashMap, HashSet};

pub struct Constraints {
    pub alphabet: Vec<Key>,
    pub elements: usize,
    pub fixed: HashSet<Element>,
    pub narrowed: HashMap<Element, Vec<Key>>,
}

impl Constraints {
    /// 传入配置表示来构造约束，把用户在配置文件中编写的约束「编译」成便于快速计算的数据结构
    pub fn new(representation: &Representation) -> Result<Constraints, Error> {
        let elements = representation.initial.len();
        let alphabet = representation
            .config
            .form
            .alphabet
            .chars()
            .map(|x| *representation.key_repr.get(&x).unwrap()) // 在生成表示的时候已经确保了这里一定有对应的键
            .collect();
        let mut fixed: HashSet<Element> = HashSet::new();
        let mut narrowed: HashMap<Element, Vec<Key>> = HashMap::new();
        let mut values: Vec<AtomicConstraint> = Vec::new();
        let lookup = |x: String| {
            let element_number = representation.element_repr.get(&x);
            element_number.ok_or(format!("{x} 不存在于键盘映射中"))
        };
        if let Some(constraints) = &representation.config.optimization.constraints {
            values.append(&mut constraints.elements.clone().unwrap_or_default());
            values.append(&mut constraints.indices.clone().unwrap_or_default());
            values.append(&mut constraints.element_indices.clone().unwrap_or_default());
        }
        let mapping = &representation.config.form.mapping;
        for atomic_constraint in &values {
            let AtomicConstraint {
                element,
                index,
                keys,
            } = atomic_constraint;
            let elements: Vec<usize> = match (element, index) {
                // 如果指定了元素和码位
                (Some(element), Some(index)) => {
                    let element = *lookup(assemble(element, *index))?;
                    vec![element]
                }
                // 如果指定了码位
                (None, Some(index)) => {
                    let mut elements = Vec::new();
                    for (key, value) in mapping {
                        let normalized = value.normalize();
                        if let Some(MappedKey::Ascii(_)) = normalized.get(*index) {
                            let element = *lookup(assemble(key, *index))?;
                            elements.push(element);
                        }
                    }
                    elements
                }
                // 如果指定了元素
                (Some(element), None) => {
                    let mapped = mapping
                        .get(element)
                        .ok_or(format!("约束中的元素 {element} 不在键盘映射中"))?;
                    let mut elements = Vec::new();
                    for (i, x) in mapped.normalize().iter().enumerate() {
                        if let MappedKey::Ascii(_) = x {
                            elements.push(*lookup(assemble(element, i))?);
                        }
                    }
                    elements
                }
                _ => return Err("约束必须至少提供 element 或 index 之一".into()),
            };
            for element in elements {
                if let Some(keys) = keys {
                    let mut transformed = Vec::new();
                    for key in keys {
                        transformed.push(
                            *representation
                                .key_repr
                                .get(key)
                                .ok_or(format!("约束中的键 {key} 不在键盘映射中"))?,
                        );
                    }
                    if transformed.len() == 0 {
                        return Err("约束中的键列表不能为空".into());
                    }
                    narrowed.insert(element, transformed);
                } else {
                    fixed.insert(element);
                }
            }
        }
        Ok(Constraints {
            alphabet,
            elements,
            fixed,
            narrowed,
        })
    }

    fn get_movable_element(&self) -> usize {
        let mut rng = thread_rng();
        loop {
            let key = rng.gen_range(0..self.elements);
            if !self.fixed.contains(&key) {
                return key;
            }
        }
    }

    fn get_swappable_element(&self) -> usize {
        let mut rng = thread_rng();
        loop {
            let key = rng.gen_range(0..self.elements);
            if !self.fixed.contains(&key) {
                return key;
            }
        }
    }

    fn swap_narrowed_elements(&self, map: &KeyMap, element1: Element, element2: Element) -> KeyMap {
        let mut next = map.clone();
        let destinations1 = self.narrowed.get(&element1).unwrap_or(&self.alphabet);
        let destinations2 = self.narrowed.get(&element2).unwrap_or(&self.alphabet);
        //分开判断可行性。这样如果无法交换，至少移动一下。
        if destinations1.contains(&map[element2]) {
            next[element1] = map[element2];
        }
        if destinations2.contains(&map[element1]) {
            next[element2] = map[element1];
        }
        next
    }

    pub fn constrained_random_swap(&self, map: &KeyMap) -> KeyMap {
        let element1 = self.get_swappable_element();
        let element2 = self.get_swappable_element();
        self.swap_narrowed_elements(map, element1, element2)
    }

    pub fn constrained_full_key_swap(&self, map: &KeyMap) -> KeyMap {
        let mut rng = thread_rng();
        let mut next = map.clone();
        // 寻找一个可移动元素和一个它的可行移动位置，然后把这两个键上的所有元素交换
        // 这样交换不成也至少能移动一次
        let movable_element = self.get_movable_element();
        let key1 = map[movable_element];
        let destinations = self
            .narrowed
            .get(&movable_element)
            .unwrap_or(&self.alphabet);
        let key2 = destinations.choose(&mut rng).unwrap(); // 在编译约束时已经确保了这里一定有可行的移动位置
        for (element, key) in map.iter().enumerate() {
            if (*key == key1 || *key == *key2) && !self.fixed.contains(&element) {
                let destination = if *key == *key2 { key1 } else { *key2 };
                //将元素移动到目标
                //考虑到组合中的元素必然在同样的键上，有同样的约束条件，也必然跟随移动，这里不再判断组合
                let destinations2 = self.narrowed.get(&element).unwrap_or(&self.alphabet);
                if destinations2.contains(&destination) {
                    next[element] = destination;
                }
            }
        }
        next
    }

    pub fn constrained_random_move(&self, map: &KeyMap) -> KeyMap {
        let mut rng = thread_rng();
        let mut next = map.clone();
        let movable_element = self.get_movable_element();
        let destinations = self
            .narrowed
            .get(&movable_element)
            .unwrap_or(&self.alphabet);
        let key = destinations.choose(&mut rng).unwrap(); // 在编译约束时已经确保了这里一定有可行的移动位置
        next[movable_element] = *key;
        next
    }
}
