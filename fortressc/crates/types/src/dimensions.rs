//! `dim` and `unit` declarations: registered, checked for well-formedness, and
//! carried no further.
//!
//! WHAT IS CHECKED IS THE WHOLE POINT. `dimensions.tex:206-215` makes a unit
//! mismatch a STATIC error, so "a phantom layer erased before codegen" is only
//! honest if the checking is there -- parse-and-erase would make `meter +
//! second` compile at exit 0, which is the silent-acceptance class this project
//! already hunts (the `where` clause that was a token skip, `excludes`
//! unenforced, `comprises` names unresolved).
//!
//! So sub-phase 4d lands its first rung and NOTHING ABOVE IT: names are
//! declared, every name in a derivation must resolve, and a dimension or unit
//! reaching a VALUE's type is refused by name at `Registry::resolve`. Dimension
//! EQUALITY, the free abelian group of `defining-dimensions.tex:42-57`, and
//! `in` conversion factors are not built, and each is refused by a diagnostic
//! that names itself rather than by silence.
//!
//! 1.0 SHIPPED THE SAME RUNG. `dimensions.tex:15-17` says the feature is not
//! supported and its examples are not run; the `.rats` grammar parses the whole
//! surface; and not one of the corpus files that declare a dimension has a
//! `.test` recording what the legacy did with it.

use std::collections::{BTreeMap, BTreeSet};

use fortress_ast::{Component, Decl, DimExpr, Span};

use crate::error::TypeError;

/// `defining-dimensions.tex:264-321`. NOT GENERATED, by decision: 175 literal
/// unit names would become 3675, which puts about eighteen hundred names in
/// one component's scope before anything can use them. They are listed only so
/// that `kilogram` can be refused with the REASON rather than as a name the
/// reader is sent looking for -- which matters immediately, because
/// `Fortress.SIUnits.fsi:17` writes `dim Mass default kilogram` and `kilogram`
/// is `gram` with a prefix.
const SI_PREFIXES: [&str; 20] = [
    "yotta", "zetta", "exa", "peta", "tera", "giga", "mega", "kilo", "hecto", "deka", "deci",
    "centi", "milli", "micro", "nano", "pico", "femto", "atto", "zepto", "yocto",
];

/// The two names that are dimensions without being declared:
/// `dimensions.tex:36-37` gives the dimensionless one two spellings.
pub(crate) const UNITY: [&str; 2] = ["Unity", "dimensionless"];

/// Every dimension and unit name the component declares. Built once and read
/// by `Registry::resolve`, which is what turns `x: Length` from
/// `unknown type` -- a diagnostic sending the reader to look for a declaration
/// that IS there -- into one that names the mechanism.
#[derive(Debug, Default)]
pub struct Dimensions {
    pub(crate) dims: BTreeSet<String>,
    pub(crate) units: BTreeSet<String>,
}

impl Dimensions {
    #[must_use]
    pub fn of(component: &Component) -> Self {
        let mut dims: BTreeSet<String> = component.dims.iter().map(|d| d.name.clone()).collect();
        for name in UNITY {
            dims.insert(name.to_owned());
        }
        let units = component
            .units
            .iter()
            .flat_map(|u| u.names.iter().cloned())
            .collect();
        Self { dims, units }
    }

    pub(crate) fn describes(&self, name: &str) -> Option<&'static str> {
        if self.dims.contains(name) {
            return Some("a dimension");
        }
        if self.units.contains(name) {
            return Some("a unit");
        }
        None
    }
}

/// TWO PASSES, AND THE ORDER IS NOT A STYLE CHOICE. `Fortress.SIUnits.fsi:29`
/// is `dim Force = Mass Acceleration` and `Acceleration` is declared
/// twenty-five lines further down, so a single forward pass would refuse the
/// library's own file for a name that is there.
pub fn check(component: &Component) -> Result<(), TypeError> {
    let known = Dimensions::of(component);

    let mut declared: BTreeMap<&str, Span> = BTreeMap::new();
    for decl in &component.decls {
        let name = match decl {
            Decl::Trait(t) => &t.name,
            Decl::Object(o) => &o.name,
            Decl::Function(f) => &f.name,
        };
        declared.entry(name).or_insert_with(|| span_of(decl));
    }

    let mut seen: BTreeMap<&str, Span> = BTreeMap::new();
    for dim in &component.dims {
        claim(&mut seen, &dim.name, dim.span, "dimension")?;
        if declared.contains_key(dim.name.as_str()) {
            return Err(TypeError::DimensionNameCollides {
                span: dim.span,
                name: dim.name.clone(),
                kind: "dimension",
            });
        }
        if let Some(derivation) = &dim.derivation {
            resolve_names(derivation, &known.dims, "dimension")?;
        }
        if let Some(unit) = &dim.default_unit {
            if !known.units.contains(unit) {
                return Err(unknown(dim.span, unit.clone(), "unit", &known.units));
            }
        }
    }
    for unit in &component.units {
        for name in &unit.names {
            claim(&mut seen, name, unit.span, "unit")?;
            if declared.contains_key(name.as_str()) {
                return Err(TypeError::DimensionNameCollides {
                    span: unit.span,
                    name: name.clone(),
                    kind: "unit",
                });
            }
        }
        if let Some(dimension) = &unit.dimension {
            if !known.dims.contains(dimension) {
                return Err(unknown(
                    unit.span,
                    dimension.clone(),
                    "dimension",
                    &known.dims,
                ));
            }
        }
        // A UNIT's right-hand side is a product of UNITS, a DIMENSION's of
        // DIMENSIONS, and the same grammar parses both -- so which namespace a
        // name must be in is decided HERE, by the position it was written in,
        // and nowhere else.
        if let Some(definition) = &unit.definition {
            resolve_names(definition, &known.units, "unit")?;
        }
    }
    Ok(())
}

fn claim<'a>(
    seen: &mut BTreeMap<&'a str, Span>,
    name: &'a str,
    span: Span,
    kind: &'static str,
) -> Result<(), TypeError> {
    if seen.insert(name, span).is_some() {
        return Err(TypeError::DimensionDeclaredTwice {
            span,
            name: name.to_owned(),
            kind,
        });
    }
    Ok(())
}

fn resolve_names(
    expr: &DimExpr,
    known: &BTreeSet<String>,
    wanted: &'static str,
) -> Result<(), TypeError> {
    let mut names = Vec::new();
    expr.names(&mut names);
    for (name, span) in names {
        if known.contains(&name) || UNITY.contains(&name.as_str()) {
            continue;
        }
        return Err(unknown(span, name, wanted, known));
    }
    Ok(())
}

/// The diagnostic, plus the one thing worth saying about WHY a name that looks
/// declared is not.
pub(crate) fn unknown(
    span: Span,
    name: String,
    wanted: &'static str,
    known: &BTreeSet<String>,
) -> TypeError {
    let prefixed = SI_PREFIXES
        .iter()
        .find_map(|p| name.strip_prefix(p).filter(|rest| known.contains(*rest)))
        .is_some();
    TypeError::UnknownDimensionName {
        span,
        name,
        wanted,
        prefixed,
    }
}

fn span_of(decl: &Decl) -> Span {
    match decl {
        Decl::Trait(t) => t.span,
        Decl::Object(o) => o.span,
        Decl::Function(f) => f.span,
    }
}
