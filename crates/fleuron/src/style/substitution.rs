//! Custom properties in the cascade: what each name holds on one
//! element, and what a value that reads one comes to.

use std::collections::{BTreeMap, BTreeSet};

use crate::Warning;

use super::properties::{ComputedStyle, Custom, Pending};
use super::sheet::{self, PROPERTIES, Spec, Unresolved};

/// The custom properties of one element: the ones it inherited, with
/// the ones its own rules declare, in cascade order, over them.
///
/// A custom property whose value names one that holds nothing, or
/// that depends on itself, holds nothing.
pub(super) fn resolve(
    inherited: &BTreeMap<String, String>,
    declared: &[&Custom],
    warnings: &mut Vec<Warning>,
) -> BTreeMap<String, String> {
    let own: BTreeMap<&str, &Custom> = declared
        .iter()
        .map(|custom| (custom.name.as_str(), *custom))
        .collect();
    let mut resolver = Resolver {
        own: &own,
        inherited,
        done: BTreeMap::new(),
        stack: Vec::new(),
        cyclic: BTreeSet::new(),
        warnings,
    };
    let mut resolved = inherited.clone();
    for name in own.keys() {
        match resolver.value(name) {
            Some(value) => resolved.insert(name.to_string(), value),
            None => resolved.remove(*name),
        };
    }
    resolved
}

struct Resolver<'a, 'w> {
    own: &'a BTreeMap<&'a str, &'a Custom>,
    inherited: &'a BTreeMap<String, String>,
    done: BTreeMap<&'a str, Option<String>>,
    /// The names being resolved, outermost first.
    stack: Vec<&'a str>,
    /// Every name found on a cycle.
    cyclic: BTreeSet<&'a str>,
    warnings: &'w mut Vec<Warning>,
}

impl<'a> Resolver<'a, '_> {
    fn value(&mut self, name: &str) -> Option<String> {
        let own = self.own;
        let Some(custom) = own.get(name) else {
            return self.inherited.get(name).cloned();
        };
        if let Some(done) = self.done.get(name) {
            return done.clone();
        }
        let name = custom.name.as_str();
        if let Some(at) = self.stack.iter().position(|open| *open == name) {
            self.cyclic.extend(&self.stack[at..]);
            return None;
        }
        self.stack.push(name);
        let substituted = sheet::substitute(&custom.value, &mut |name| self.value(name));
        self.stack.pop();
        let value = if self.cyclic.contains(name) {
            warn_once(
                self.warnings,
                format!("Custom property `{name}` depends on itself. `{name}` has no value."),
                &custom.origin,
            );
            None
        } else {
            substituted.ok()
        };
        self.done.insert(name, value.clone());
        value
    }
}

/// What `pending` comes to under the custom properties in `custom`,
/// read by `read`. `None` is a value that became nothing, and the
/// warning that says so is already made. The caller then resets
/// every longhand the property sets.
pub(super) fn substituted<D>(
    pending: &Pending,
    custom: &BTreeMap<String, String>,
    read: fn(&Pending, &str) -> Option<Vec<D>>,
    warnings: &mut Vec<Warning>,
) -> Option<Vec<D>> {
    let why = match sheet::substitute(&pending.value, &mut |name| custom.get(name).cloned()) {
        Ok(css) => match read(pending, &css) {
            Some(declarations) => return Some(declarations),
            None => Unresolved::Unsupported,
        },
        Err(why) => why,
    };
    let property = &pending.property;
    let problem = match why {
        Unresolved::Missing(name) => format!("Custom property `{name}` has no value."),
        Unresolved::Unsupported => format!("Unsupported value for `{property}`."),
    };
    let inherits = Spec::find(PROPERTIES, property).is_some_and(|spec| spec.inherited);
    let takes = if inherits {
        "its inherited value"
    } else {
        "its initial value"
    };
    warn_once(
        warnings,
        format!("{problem} `{property}` takes {takes}."),
        &pending.origin,
    );
    None
}

/// Applies a pending declaration of a style rule. `base` is the style
/// before any declaration applied, which is what a value that became
/// nothing leaves each longhand at.
pub(super) fn apply(
    style: &mut ComputedStyle,
    pending: &Pending,
    base: &ComputedStyle,
    parent_size: f32,
    root_size: f32,
    warnings: &mut Vec<Warning>,
) {
    match substituted(pending, &style.custom, sheet::read_pending, warnings) {
        Some(declarations) => {
            for declaration in &declarations {
                style.apply(declaration, parent_size, root_size);
            }
        }
        None => {
            for like in sheet::longhands(&pending.property) {
                style.reset(&like, base);
            }
        }
    }
}

/// Records a warning once. Every element a rule matches reads the
/// same declaration, and the declaration is what the author fixes.
fn warn_once(warnings: &mut Vec<Warning>, message: String, origin: &str) {
    let seen = warnings
        .iter()
        .any(|seen| seen.message == message && seen.origin.as_deref() == Some(origin));
    if !seen {
        warnings.push(Warning {
            message,
            origin: Some(origin.to_string()),
        });
    }
}
