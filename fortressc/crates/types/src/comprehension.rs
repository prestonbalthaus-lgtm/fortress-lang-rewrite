//! `<| e | x <- lo:hi, guard |>` and `{ e | x <- lo:hi, guard }`, lowered to a
//! real `List[\T\]` and a real `Set[\T\]`.
//!
//! ROUTE 4, and the whole of it is here: an AST-to-AST pass that rewrites the
//! comprehension into an ordinary block over a minted `List[\T\]` object, and
//! splices that object's declaration in when something demanded it. It runs
//! BEFORE `mono::expand`, so `List[\ZZ64\]` is stamped like any other
//! instantiation and monomorphization does all the work.
//!
//! WHAT THE LOWERING IS:
//!
//! ```text
//! <|[\T\] e | x <- lo:hi, p |>
//!
//!   do
//!     acc$0 = List[\T\](0)
//!     i$0: ZZ64 := lo
//!     while i$0 <= hi do
//!       x = i$0
//!       if p then acc$0.append(e) end
//!       i$0 := i$0 + 1
//!     end
//!     acc$0
//!   end
//! ```
//!
//! SEQUENTIAL, AND THAT IS A NAMED DEVIATION. 1.0 defines a comprehension as a
//! `BIG` reduction, which is parallel unless every generator is `seq`. A `while`
//! is used here rather than a `for` for two reasons that are the same reason: a
//! `for` body is OUTLINED and its iterations may run on several workers, so
//! appending to one shared list would be a data race, and the ORDER of a list
//! comprehension's result is defined by the generator. Getting the parallel
//! version right needs an associative CONCAT reduction over a list monoid,
//! which is a milestone and not a lowering.
//!
//! THE ELEMENT TYPE IS WRITTEN, NEVER INFERRED, exactly as a static argument is
//! everywhere else in this compiler. It comes from the comprehension's own
//! `[\T\]` -- which is 1.0's spelling, `parser_tests/XXXPreparser.ad.fss`
//! writes `<|[\ZZ32\] x | x <- xset` -- or from the written type of the binding
//! it initialises. Neither, and it is refused by name.

use fortress_ast::{
    Assign, Binding, BlockItem, Component, Decl, Expr, GeneratorClause, Member, Span, TypeRef,
};

use crate::error::TypeError;

/// The minted collections. Parsed, never linked: only their declarations are
/// spliced, and only into a component that used a comprehension of that shape.
const LIST_SOURCE: &str = include_str!("List.fss");
const SET_SOURCE: &str = include_str!("Set.fss");
const MAP_SOURCE: &str = include_str!("Map.fss");

/// One collection a comprehension can build. The bracket pair is how
/// `enclosing_application` names the form; the builder is the method the
/// lowering calls once per element that survives the guards, and it is the
/// ONLY thing that differs between the two lowerings -- `append` keeps every
/// element, `insert` keeps the first of each.
struct Kind {
    bracket: &'static str,
    name: &'static str,
    source: &'static str,
    builder: &'static str,
    /// How many static arguments the collection takes, and therefore how many
    /// expressions one element contributes: a `List` and a `Set` hold ONE
    /// value per element, a `Map` holds a key AND a value.
    arity: usize,
}

const KINDS: [Kind; 3] = [
    Kind {
        bracket: "<|_|>",
        name: "List",
        source: LIST_SOURCE,
        builder: "append",
        arity: 1,
    },
    Kind {
        bracket: "{_}",
        name: "Set",
        source: SET_SOURCE,
        builder: "insert",
        arity: 1,
    },
    // THE SAME BRACKET AS THE SET, AND THE ELEMENT DECIDES WHICH. `{a, b}` is
    // a set and `{k |-> v}` is a map; 1.0 spells them with one encloser too
    // (`Library/Set.fsi:55` and `Library/Map.fsi`) and tells them apart the
    // same way. That is why `kind_for` takes the shape of the element and not
    // just the brackets.
    Kind {
        bracket: "{_}",
        name: "Map",
        source: MAP_SOURCE,
        builder: "insert",
        arity: 2,
    },
];

const SET: &str = "Set";

/// The enclosing-operator NAME a set literal parses to. `enclosed` builds
/// `open + "_" + close`, so `{a, b, c}` is a CALL to a function called `{_}`
/// with the elements as its arguments -- there is no literal node to match on,
/// and that is what this pass looks for.
const SET_LITERAL: &str = "{_}";

/// Which collection a bracket pair builds. `mapping` is whether the element --
/// a literal's argument, or a comprehension's body -- is written `k |-> v`.
fn kind_for(bracket: &str, mapping: bool) -> Option<&'static Kind> {
    let wanted = if mapping { 2 } else { 1 };
    KINDS
        .iter()
        .find(|k| k.bracket == bracket && k.arity == wanted)
}

/// A map form written with a bracket that has no map collection behind it --
/// `<| k |-> v |>`. Told apart from an ordinary unsupported bracket so the
/// diagnostic can say which half is the problem.
fn is_mapping(e: &Expr) -> bool {
    matches!(e, Expr::Mapping { .. })
}

pub fn lower(component: &Component) -> Result<Component, TypeError> {
    let mut pass = Pass {
        counter: 0,
        demanded: [false; KINDS.len()],
    };
    let mut out = component.clone();
    for decl in &mut out.decls {
        pass.decl(decl)?;
    }
    for (kind, demanded) in KINDS.iter().zip(pass.demanded.iter()) {
        if !*demanded {
            continue;
        }
        // THE FILE'S OWN DECLARATION WINS AND A MERGED ONE LOSES, which is
        // LINK 5's RULE 1 ONE LEVEL DOWN -- a merged declaration loses to a
        // builtin of its own name, and the minted collection is morally a
        // builtin: it is what `<|..|>` and `{..}` are DEFINED to build.
        //
        // AND THE MERGED ONE COULD NEVER HAVE BEEN USED ANYWAY. An api
        // DECLARES and a component DEFINES, so an imported `List` or `Set` is
        // `MergedObjectNotConstructible` -- there is no way to write one down.
        // Replacing it with a constructible object of the same name and arity
        // strictly widens what a program can do; refusing instead is what took
        // `Test3.fss`, `FunctionalMethodAsUnifyParam.fss` and `importBig.fss`
        // down, all three for writing `import List.{...}` beside a
        // comprehension.
        //
        // A DECLARATION THE FILE WROTE ITSELF IS STILL A REFUSAL, because that
        // one IS constructible and the program means it. `badcomplisttaken
        // .fss` is the fixture and it declares `object List(n: ZZ64)`.
        if out.decls.iter().any(|d| collides(d, kind.name)) {
            return Err(TypeError::ComprehensionNameTaken {
                span: component.span,
                name: kind.name,
            });
        }
        out.decls.retain(|d| named(d) != Some(kind.name));
        out.decls.extend(minted(kind.source));
    }
    Ok(out)
}

/// The collision that is a REFUSAL, on ONE LINE AND WITH NO VERTICAL BAR in
/// it: a mutation row splits on `IFS='|'`, so the predicate a table has to
/// reach cannot live inside a closure. Dropping the `merged` half is that
/// row, and it puts the three corpus files back on the floor.
fn collides(decl: &Decl, name: &str) -> bool {
    named(decl) == Some(name) && !merged(decl)
}

/// Whether a declaration came out of an imported api rather than out of this
/// file. Only a TRAIT or an OBJECT carries the flag, which is exactly the two
/// shapes a collection name can collide with.
fn merged(decl: &Decl) -> bool {
    match decl {
        Decl::Trait(t) => t.merged,
        Decl::Object(o) => o.merged,
        Decl::Function(_) | Decl::Value(_) => false,
    }
}

fn named(decl: &Decl) -> Option<&str> {
    match decl {
        Decl::Trait(t) => Some(t.name.as_str()),
        Decl::Object(o) => Some(o.name.as_str()),
        Decl::Function(f) => Some(f.name.as_str()),
        Decl::Value(v) => Some(v.name.as_str()),
    }
}

/// A minted collection's declarations. Parsed once per component that needs
/// them; each file is a `component` so it reads as ordinary Fortress rather
/// than as a fragment with no home.
fn minted(source: &str) -> Vec<Decl> {
    let tokens = match fortress_lexer::lex(source) {
        Ok(t) => t,
        // Unreachable: the source is a constant in this crate, and
        // `minted_sources_parse` is the test that says so.
        Err(_) => return Vec::new(),
    };
    match fortress_parser::parse(&tokens) {
        Ok(c) => c.decls,
        Err(_) => Vec::new(),
    }
}

struct Pass {
    counter: usize,
    demanded: [bool; KINDS.len()],
}

impl Pass {
    fn decl(&mut self, decl: &mut Decl) -> Result<(), TypeError> {
        match decl {
            Decl::Function(f) => {
                let wanted = f.return_type.clone();
                if let Some(body) = &mut f.body {
                    self.expr(body, wanted.as_ref())?;
                }
            }
            Decl::Value(v) => {
                if let Some(init) = &mut v.init {
                    let wanted = v.ty.clone();
                    self.expr(init, wanted.as_ref())?;
                }
            }
            Decl::Trait(t) => {
                for m in &mut t.members {
                    self.member(m)?;
                }
            }
            Decl::Object(o) => {
                for m in &mut o.members {
                    self.member(m)?;
                }
            }
        }
        Ok(())
    }

    fn member(&mut self, member: &mut Member) -> Result<(), TypeError> {
        match member {
            Member::Method(m) => {
                let wanted = m.return_type.clone();
                if let Some(body) = &mut m.body {
                    self.expr(body, wanted.as_ref())?;
                }
            }
            Member::Field(f) => {
                if let Some(init) = &mut f.init {
                    let wanted = f.ty.clone();
                    self.expr(init, Some(&wanted))?;
                }
            }
            Member::Coercion { .. } => {}
        }
        Ok(())
    }

    /// `wanted` is the written type of the slot this expression initialises,
    /// and it is consulted for ONE thing: a comprehension's element type.
    fn expr(&mut self, e: &mut Expr, wanted: Option<&TypeRef>) -> Result<(), TypeError> {
        if matches!(e, Expr::Comprehension { .. }) {
            return self.comprehension(e, wanted);
        }
        // A SET LITERAL IS A `Call`, NOT A LITERAL NODE, so it has to be tried
        // before the ordinary Call walk -- `set_literal` reports whether it
        // took the node, and having taken it, it has already walked the
        // elements. Falling through would walk the REWRITTEN block a second
        // time, which is harmless today and is exactly the kind of double walk
        // that stops being harmless the moment a pass counts something.
        if self.set_literal(e, wanted)? {
            return Ok(());
        }
        self.children(e)
    }

    /// EXHAUSTIVE ON PURPOSE. A catch-all here would swallow the next `Expr`
    /// variant silently and leave a comprehension inside it un-lowered, which
    /// arrives as `a comprehension parses and its lowering is not implemented`
    /// on a program that writes an ordinary one. E0004 is the instrument.
    fn children(&mut self, e: &mut Expr) -> Result<(), TypeError> {
        match e {
            Expr::Unit { .. }
            | Expr::IntLit { .. }
            | Expr::FloatLit { .. }
            | Expr::StrLit { .. }
            | Expr::CharLit { .. }
            | Expr::BoolLit { .. }
            | Expr::Var { .. }
            | Expr::Exit { value: None, .. } => {}
            Expr::Comprehension { .. } => self.comprehension(e, None)?,
            // A MAPPING REACHES HERE ONLY OUTSIDE A MAP FORM. Inside one, the
            // literal and the comprehension take its halves apart themselves
            // and this arm never sees it; anywhere else it is refused by the
            // checker, because `k |-> v` is one entry of a map and not a value.
            Expr::Mapping { key, value, .. } => {
                self.expr(key, None)?;
                self.expr(value, None)?;
            }
            Expr::Tuple { items, .. } | Expr::Juxt { items, .. } => {
                for item in items {
                    self.expr(item, None)?;
                }
            }
            Expr::ArrayLit { items, .. } => {
                for item in items {
                    self.expr(item, None)?;
                }
            }
            Expr::Infix { lhs, rhs, .. } => {
                self.expr(lhs, None)?;
                self.expr(rhs, None)?;
            }
            Expr::Prefix { operand: inner, .. }
            | Expr::Throw { value: inner, .. }
            | Expr::Annotated { value: inner, .. }
            | Expr::Field { base: inner, .. }
            | Expr::Atomic { body: inner, .. }
            | Expr::Spawn { body: inner, .. }
            | Expr::Label { body: inner, .. }
            | Expr::Lambda { body: inner, .. }
            | Expr::Instantiate { callee: inner, .. }
            | Expr::Exit {
                value: Some(inner), ..
            } => self.expr(inner, None)?,
            Expr::Call { callee, args, .. } => {
                self.expr(callee, None)?;
                for arg in args {
                    self.expr(arg, None)?;
                }
            }
            Expr::Index { base, indices, .. } => {
                self.expr(base, None)?;
                for index in indices {
                    self.expr(index, None)?;
                }
            }
            Expr::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                self.expr(cond, None)?;
                self.expr(then_branch, None)?;
                if let Some(otherwise) = else_branch {
                    self.expr(otherwise, None)?;
                }
            }
            Expr::While { cond, body, .. } => {
                self.expr(cond, None)?;
                self.expr(body, None)?;
            }
            Expr::BindingCondition {
                source,
                body,
                otherwise,
                ..
            } => {
                self.expr(source, None)?;
                self.expr(body, None)?;
                if let Some(o) = otherwise {
                    self.expr(o, None)?;
                }
            }
            Expr::Block { items, .. } => {
                for item in items {
                    match item {
                        BlockItem::Binding(b) => {
                            let wanted = b.ty.clone();
                            self.expr(&mut b.value, wanted.as_ref())?;
                        }
                        BlockItem::TupleBinding(b) => self.expr(&mut b.value, None)?,
                        BlockItem::Assign(a) => {
                            self.expr(&mut a.target, None)?;
                            self.expr(&mut a.value, None)?;
                        }
                        BlockItem::Expr(x) => self.expr(x, None)?,
                    }
                }
            }
            Expr::ObjectExpr { members, .. } => {
                for member in members {
                    self.member(member)?;
                }
            }
            Expr::Try {
                body,
                arms,
                finally,
                ..
            } => {
                self.expr(body, None)?;
                for arm in arms {
                    self.expr(&mut arm.body, None)?;
                }
                if let Some(f) = finally {
                    self.expr(f, None)?;
                }
            }
            Expr::For { lo, hi, body, .. } | Expr::BigReduction { lo, hi, body, .. } => {
                self.expr(lo, None)?;
                self.expr(hi, None)?;
                self.expr(body, None)?;
            }
            Expr::ForIn { source, body, .. } | Expr::SeqIterate { source, body, .. } => {
                self.expr(source, None)?;
                self.expr(body, None)?;
            }
            Expr::AlsoDo { blocks, .. } => {
                for block in blocks {
                    self.expr(block, None)?;
                }
            }
            Expr::Case {
                subject,
                arms,
                else_arm,
                ..
            } => {
                self.expr(subject, None)?;
                for arm in arms {
                    self.expr(&mut arm.guard, None)?;
                    self.expr(&mut arm.body, None)?;
                }
                if let Some(otherwise) = else_arm {
                    self.expr(otherwise, None)?;
                }
            }
            Expr::TypeCase {
                subject,
                arms,
                else_arm,
                ..
            } => {
                self.expr(subject, None)?;
                for arm in arms {
                    self.expr(&mut arm.body, None)?;
                }
                self.expr(else_arm, None)?;
            }
        }
        Ok(())
    }

    /// `{a, b, c}`, lowered onto the same minted `Set[\T\]` the comprehension
    /// builds:
    ///
    /// ```text
    ///   do
    ///     acc$0 = Set[\T\](0)
    ///     acc$0.insert(a)
    ///     acc$0.insert(b)
    ///     acc$0.insert(c)
    ///     acc$0
    ///   end
    /// ```
    ///
    /// THE ELEMENT TYPE IS WRITTEN OR IT COMES OFF THE SLOT, and there is no
    /// third option HERE FOR A STRUCTURAL REASON rather than a stylistic one.
    /// This pass runs before `mono::expand`, which is what STAMPS
    /// `Set[\ZZ32\]`; a literal whose element type is only discoverable by
    /// TYPING its elements cannot be stamped, because there are no types yet
    /// and the checker that would make them runs after expansion has frozen
    /// the concrete set. `SeqIterate` is not a precedent for doing it later --
    /// that walks a collection that already exists, and mints nothing.
    fn set_literal(&mut self, e: &mut Expr, wanted: Option<&TypeRef>) -> Result<bool, TypeError> {
        let Expr::Call { callee, args, span } = e else {
            return Ok(false);
        };
        let span = *span;
        let (written, is_literal) = match &**callee {
            Expr::Var { name, .. } => (Vec::new(), name == SET_LITERAL),
            Expr::Instantiate {
                callee: inner,
                args: statics,
                ..
            } => match &**inner {
                Expr::Var { name, .. } if name == SET_LITERAL => (statics.clone(), true),
                _ => (Vec::new(), false),
            },
            _ => (Vec::new(), false),
        };
        if !is_literal {
            return Ok(false);
        }
        let mapping = args.iter().any(is_mapping);
        // A LITERAL MAY NOT MIX THE TWO. `{1, k |-> v}` is neither a set nor a
        // map, and reading it as either drops half of what was written.
        if mapping && !args.iter().all(is_mapping) {
            return Err(TypeError::MappingOutsideAMap { span });
        }
        let Some(kind) = kind_for(SET_LITERAL, mapping) else {
            return Ok(false);
        };
        // NAMED `from_slot` AND NOT `slot`, AND THE DEMAND MARKING IS A HELPER,
        // for the same reason: `tools/mutation-patterns.py` matches a row's
        // pattern with `grep -F` and a row must hit EXACTLY ONCE. Written the
        // obvious way, these two lines are byte-identical to the ones in
        // `comprehension` above and apply-gate's rows silently stopped being
        // unique -- reported as "could not be applied", never as a failure.
        let statics = match (written.is_empty(), wanted.and_then(|w| args_of(kind, w))) {
            (false, _) => written,
            (true, Some(from_slot)) => from_slot,
            (true, None) => return Err(TypeError::SetLiteralElementUnwritten { span }),
        };
        if statics.len() != kind.arity {
            return Err(TypeError::SetLiteralElementUnwritten { span });
        }
        let mut elements = std::mem::take(args);
        for element_expr in &mut elements {
            self.expr(element_expr, None)?;
        }
        let index = self.counter;
        self.counter = self.counter.saturating_add(1);
        self.demand(kind);
        let acc = format!("acc${index}");
        let mut items = vec![BlockItem::Binding(Binding {
            name: acc.clone(),
            ty: None,
            value: call(
                Expr::Instantiate {
                    callee: Box::new(var(kind.name, span)),
                    args: statics,
                    span,
                },
                vec![zero(span)],
                span,
            ),
            mutable: false,
            span,
        })];
        for element_expr in elements {
            // ONE ELEMENT, ONE `insert`, AND A MAPPING CONTRIBUTES TWO
            // ARGUMENTS. The mapping is taken apart HERE and never lowered as
            // a value, which is what `MappingOutsideAMap` says.
            let built = match element_expr {
                Expr::Mapping { key, value, .. } => vec![*key, *value],
                other => vec![other],
            };
            items.push(BlockItem::Expr(call(
                field(var(&acc, span), kind.builder, span),
                built,
                span,
            )));
        }
        items.push(BlockItem::Expr(var(&acc, span)));
        *e = Expr::Block { items, span };
        Ok(true)
    }

    /// Record that this component needs one collection minted. ZIPPED RATHER
    /// THAN INDEXED, because `clippy::indexing_slicing` is denied here and a
    /// compiler pass is the last place that wants a panicking index; the two
    /// arrays are the same length by construction. ONE function rather than
    /// two copies of the loop, so the mutation row that clears the flag has
    /// exactly one place to match.
    fn demand(&mut self, kind: &Kind) {
        for (k, flag) in KINDS.iter().zip(self.demanded.iter_mut()) {
            if k.name == kind.name {
                *flag = true;
            }
        }
    }

    fn comprehension(&mut self, e: &mut Expr, wanted: Option<&TypeRef>) -> Result<(), TypeError> {
        let Expr::Comprehension {
            bracket,
            static_args,
            body,
            gens,
            span,
        } = e
        else {
            return Ok(());
        };
        let span = *span;
        let mapping = is_mapping(body);
        let Some(kind) = kind_for(bracket, mapping) else {
            // WHICH HALF IS UNSUPPORTED IS A DIFFERENT SENTENCE. `<| k |-> v |
            // ... |>` has a bracket that IS implemented and a BODY that has no
            // collection behind it -- there is no list of mappings -- and
            // saying "this bracket's lowering is not implemented" of a bracket
            // that plainly works sends the reader to the wrong place.
            if mapping && kind_for(bracket, false).is_some() {
                return Err(TypeError::ComprehensionGeneratorUnsupported {
                    span,
                    form: "a mapping body, which only the `{ }` brackets build,",
                });
            }
            return Err(TypeError::ComprehensionUnsupported {
                span,
                bracket: bracket.clone(),
            });
        };
        // TWO STATIC ARGUMENTS OVER A BODY THAT IS NOT A MAPPING IS NOT A MAP.
        // `not_working_static_tests/SetComprehension.fss` writes
        // `{[\ZZ32,ZZ32\] a | a<-3:10 }` -- a map's static arguments over a
        // set's body -- and it is refused BY NAME rather than let through to
        // build one collection with the other's shape.
        if kind.name == SET && static_args.len() > 1 {
            return Err(TypeError::ComprehensionGeneratorUnsupported {
                span,
                form: "a map comprehension, whose body must be written `k |-> v`,",
            });
        }
        let statics = match (
            static_args.is_empty(),
            wanted.and_then(|w| args_of(kind, w)),
        ) {
            (false, _) => static_args.clone(),
            (true, Some(slot)) => slot,
            (true, None) => return Err(TypeError::ComprehensionElementUnwritten { span }),
        };
        if statics.len() != kind.arity {
            return Err(TypeError::ComprehensionElementUnwritten { span });
        }
        let mut body = (**body).clone();
        self.expr(&mut body, None)?;
        let mut gens = std::mem::take(gens);
        for g in &mut gens {
            self.expr(&mut g.init, None)?;
            if let Some(h) = &mut g.hi {
                self.expr(h, None)?;
            }
        }

        let index = self.counter;
        self.counter = self.counter.saturating_add(1);
        self.demand(kind);
        let acc = format!("acc${index}");

        // Innermost first: each clause wraps what the ones to its right built.
        // A MAPPING BODY CONTRIBUTES TWO ARGUMENTS and is taken apart here,
        // exactly as the literal takes its elements apart. It is never lowered
        // as a value, which is what `MappingOutsideAMap` says.
        let built = match body {
            Expr::Mapping { key, value, .. } => vec![*key, *value],
            other => vec![other],
        };
        let mut inner = call(field(var(&acc, span), kind.builder, span), built, span);
        for (depth, clause) in gens.iter().enumerate().rev() {
            inner = self.clause(clause, inner, index, depth, &acc, span)?;
        }

        *e = Expr::Block {
            items: vec![
                BlockItem::Binding(Binding {
                    name: acc.clone(),
                    ty: None,
                    value: call(
                        Expr::Instantiate {
                            callee: Box::new(var(kind.name, span)),
                            args: statics,
                            span,
                        },
                        vec![zero(span)],
                        span,
                    ),
                    mutable: false,
                    span,
                }),
                BlockItem::Expr(inner),
                BlockItem::Expr(var(&acc, span)),
            ],
            span,
        };
        Ok(())
    }

    /// One generator clause, wrapped around what the clauses to its right
    /// built. A clause with no binder is a GUARD.
    fn clause(
        &mut self,
        clause: &GeneratorClause,
        inner: Expr,
        index: usize,
        depth: usize,
        acc: &str,
        span: Span,
    ) -> Result<Expr, TypeError> {
        if clause.binders.is_empty() {
            let _ = acc;
            return Ok(Expr::If {
                cond: Box::new(clause.init.clone()),
                then_branch: Box::new(inner),
                else_branch: None,
                span,
            });
        }
        let [binder] = clause.binders.as_slice() else {
            return Err(TypeError::ComprehensionGeneratorUnsupported {
                span: clause.span,
                form: "a generator binding more than one name",
            });
        };
        // A GENERATOR OVER A COLLECTION. The parser sets `hi` only for a RANGE
        // (`a:b`, `a#n`), so `None` here is a source that has to be WALKED --
        // and which members carry its extent is a question about its TYPE,
        // which this pass runs before there are any. `SeqIterate` is that
        // question handed to the checker; `Checker::seq_iterate` lowers it to
        // the same `while` shape the range arm builds below.
        let Some(hi) = clause.hi.clone() else {
            return Ok(Expr::SeqIterate {
                binder: (*binder).clone(),
                source: Box::new(clause.init.clone()),
                body: Box::new(inner),
                span,
            });
        };
        let counter = format!("i${index}${depth}");
        // `lo:hi` runs while `i <= hi`; `lo#n` runs `n` times from `lo`.
        let bound = if clause.inclusive {
            infix_le(var(&counter, span), hi, span)
        } else {
            infix_lt(
                var(&counter, span),
                infix_add(clause.init.clone(), hi, span),
                span,
            )
        };
        Ok(Expr::Block {
            items: vec![
                BlockItem::Binding(Binding {
                    name: counter.clone(),
                    ty: Some(zz64(span)),
                    value: clause.init.clone(),
                    mutable: true,
                    span,
                }),
                BlockItem::Expr(Expr::While {
                    cond: Box::new(bound),
                    body: Box::new(Expr::Block {
                        items: vec![
                            BlockItem::Binding(Binding {
                                name: (*binder).clone(),
                                ty: None,
                                value: var(&counter, span),
                                mutable: false,
                                span,
                            }),
                            BlockItem::Expr(inner),
                            BlockItem::Assign(Assign {
                                target: var(&counter, span),
                                op: None,
                                value: infix_add(var(&counter, span), one(span), span),
                                span,
                            }),
                        ],
                        span,
                    }),
                    span,
                }),
            ],
            span,
        })
    }
}

/// This kind's collection written in a slot, and nothing else: the element
/// type a comprehension takes from the binding it initialises. A `List[\T\]`
/// slot does not give a SET comprehension its element type, and the other way
/// round -- the two are different collections and reading one as the other
/// would silently build the wrong one.
fn args_of(kind: &Kind, wanted: &TypeRef) -> Option<Vec<TypeRef>> {
    match wanted {
        TypeRef::Named { name, args, .. } if name == kind.name && args.len() == kind.arity => {
            Some(args.clone())
        }
        _ => None,
    }
}

// --------------------------------------------------------------- AST helpers

fn var(name: &str, span: Span) -> Expr {
    Expr::Var {
        name: name.to_owned(),
        span,
    }
}

fn field(base: Expr, name: &str, span: Span) -> Expr {
    Expr::Field {
        base: Box::new(base),
        name: name.to_owned(),
        span,
    }
}

fn call(callee: Expr, args: Vec<Expr>, span: Span) -> Expr {
    Expr::Call {
        callee: Box::new(callee),
        args,
        span,
    }
}

fn zz64(span: Span) -> TypeRef {
    TypeRef::Named {
        name: "ZZ64".to_owned(),
        args: Vec::new(),
        span,
    }
}

fn int(text: &str, span: Span) -> Expr {
    Expr::IntLit {
        digits: text.to_owned(),
        span,
    }
}

fn zero(span: Span) -> Expr {
    int("0", span)
}

fn one(span: Span) -> Expr {
    int("1", span)
}

fn infix(op: fortress_ast::BinOp, lhs: Expr, rhs: Expr, span: Span) -> Expr {
    Expr::Infix {
        op,
        fixity: fortress_ast::Fixity::Loose,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
        span,
    }
}

fn infix_le(left: Expr, right: Expr, span: Span) -> Expr {
    infix(fortress_ast::BinOp::Le, left, right, span)
}

fn infix_lt(left: Expr, right: Expr, span: Span) -> Expr {
    infix(fortress_ast::BinOp::Lt, left, right, span)
}

fn infix_add(left: Expr, right: Expr, span: Span) -> Expr {
    infix(fortress_ast::BinOp::Add, left, right, span)
}
