//! Lowering to Cut, the shared back end's upper IR (`docs/CUT.md` in the
//! Meadow repository).  Until the back end's parser and reference
//! interpreter run it, what is checked is the text: that each construct
//! lowers to what the spec says it is.  Fresh names are numbered; the tests
//! read them with the numbers left out (`n_1` as `n_N`).

use idyllic::pipeline::{self, Options};
use idyllic_diagnostics::OutputStyle;

fn cut(src: &str) -> Result<String, Vec<String>> {
    let out = pipeline::compile(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
    assert!(!out.has_errors(), "{src}\n{:#?}", out.diagnostics);
    let mut checked = out.checked.expect("checked");
    idyllic_cut::lower::lower(&mut checked, "Main").map(|p| p.to_string())
}

/// The program as Cut, with fresh names' numbers left out.
fn lowered(src: &str) -> String {
    let text = cut(src).unwrap_or_else(|e| panic!("{src}\n{e:#?}"));
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        if c == '_' && chars.peek().is_some_and(|d| d.is_ascii_digit()) {
            while chars.peek().is_some_and(|d| d.is_ascii_digit()) {
                chars.next();
            }
            out.push('N');
        }
    }
    out
}

fn has(text: &str, part: &str) {
    assert!(text.contains(part), "expected\n{part}\nin\n{text}");
}

#[test]
fn a_function_is_a_def_and_the_expression_the_entry() {
    let text = lowered("double : Int -> Int\ndouble n = n * 2\n\ndouble 21\n");
    // its value is written out (see `an_answer_is_written_out_as_the_repl_writes_it`)
    has(&text, "cut 0\nentry idyll:Main/\"#entry\"\nanswer str\n");
    has(&text, "def idyll:Main/double (n_N: i64; k_N: ptr) =\n  prim mul(n_N, 2; k_N)");
    has(&text, "def idyll:Main/\"#entry\" (; k_N: ptr) =\n  idyll:Main/double(21; μ̃ answer_N: i64.\n    idyll:Prelude/showInt(answer_N; k_N))");
    // the prelude's effect alias is a type, nothing at run time
    assert!(!text.contains("Eff"), "{text}");
}

#[test]
fn data_is_declared_built_and_taken_apart() {
    let nat = "data Nat = Z | S Nat\n\nuse Nat.*\n\nplus : Nat -> Nat -> Nat\nplus Z m = m\nplus (S n) m = S (plus n m)\n\n";
    let text = lowered(&format!("{nat}plus (S Z) (S Z)\n"));
    has(&text, "data idyll:Main/Nat { Z; S(ptr) }");
    has(&text, "<x_N | case {\n    idyll:Main/Nat.Z =>\n      <m_N | k_N>;\n    idyll:Main/Nat.S(n_N: ptr) =>");
    has(&text, "idyll:Main/plus(n_N, m_N; μ̃ v_N: ptr.\n        <idyll:Main/Nat.S(v_N) | k_N>)");
}

#[test]
fn a_type_parameter_is_a_descriptor() {
    let text = lowered("id : {a : Type} -> a -> a\nid x = x\n\nid 3\n");
    has(&text, "def idyll:Main/id <'a_N = d_a_N> (d_a_N: desc, x_N: 'a_N; k_N: ptr) =\n  <x_N | k_N>");
    has(&text, "idyll:Main/id(desc(i64), 3; μ̃ answer_N: i64.");
}

#[test]
fn the_preludes_list_has_a_representation_variable_and_its_roles() {
    let text = lowered("sum : List Int -> Int\nsum Nil = 0\nsum (x :: xs) = x + sum xs\n\nsum (1 :: 2 :: Nil)\n");
    has(&text, "data idyll:Prelude/List <'a0> { Nil; \"::\"('a0, ptr) }");
    has(&text, "roles {\n  nil = idyll:Prelude/List.Nil\n  cons = idyll:Prelude/List.\"::\"\n}");
    has(&text, "idyll:Prelude/List.\"::\"(x_N: i64, xs_N: ptr) =>");
}

#[test]
fn values_and_the_world() {
    let text = lowered("greeting : String\ngreeting = \"hello\"\n\nmain : () -> () ! Eff\nmain () = putStr greeting\n");
    has(&text, "native {\n  idyll:Prelude/Console.putStr = Console.writeOutput\n}");
    has(&text, "val idyll:Main/greeting : str =\n  <\"hello\" | halt>");
    has(&text, "perform idyll:Prelude/Console.putStr(idyll:Main/greeting; k_N)");
    // a run of `main` answers nothing
    has(&text, "answer none");
    // a string expression is the answer
    has(&lowered("\"hi\"\n"), "answer str");
}

#[test]
fn a_bool_is_branched_on_with_if() {
    let text = lowered("if 1 < 2 then \"yes\" else \"no\"\n");
    // (the two ways meet at a join point, where the answer is written out)
    has(&text, "prim if(v__N; μ̃ u_N: unit.\n        <\"no\" | j_N>, μ̃ u_N: unit.\n        <\"yes\" | j_N>) | μ̃ answer_N: str.");
}

#[test]
fn handlers_bind_the_resumption_and_where_the_handle_goes() {
    let ask = "effect Ask where\n  ask : () -> Int\n\nsum3 : () -> Int ! Ask\nsum3 u = ask () + ask () + ask ()\n\n";
    let text = lowered(&format!("{ask}handle sum3 () with\n  ask _ k -> k 10\n"));
    has(&text, "effect idyll:Main/Ask { ask(unit) -> i64 }");
    has(&text, "perform idyll:Main/Ask.ask(unit; μ̃ v_N: i64.");
    has(&text, "idyll:Main/Ask.ask(v__N: unit; r_N: ptr, k_N: ptr) =>\n      <r_N | resume(10; k_N)>;");
    has(&text, "return(x_N: i64; k_N: ptr) =>\n      <x_N | k_N>");
    has(&text, "} in μ b_N.\n    idyll:Main/sum3(unit; b_N)\n  ; μ̃ answer_N: i64.");
    // an operation resumed more than once says so
    let choose = "effect Choose where\n  @many flip : () -> Bool\n\npick : () -> Int ! Choose\npick u = if flip () then 1 else 10\n\n";
    let text = lowered(&format!("{choose}handle pick () with\n  flip _ k -> k True + k False\n"));
    has(&text, "effect idyll:Main/Choose { @many flip(unit) -> bool }");
}

#[test]
fn a_partial_application_waits_for_the_rest() {
    let text = lowered("add : Int -> Int -> Int\nadd a b = a + b\n\nlet f = add 1 in f 2\n");
    // `add 1` is an object taking `b`, as an `i64`, then calling `add`
    has(&text, "<cocase {\n    apply(a_N: i64; k_N: ptr) =>\n      idyll:Main/add(1, a_N; k_N)\n  } | μ̃ f_N: ptr.\n    <f_N | apply(2; μ̃ answer_N: i64.");
}

#[test]
fn a_recursive_definition_inside_another_is_lifted() {
    let nat = "data Nat = Z | S Nat\n\nuse Nat.*\n\nplus : Nat -> Nat -> Nat\nplus Z m = m\nplus (S n) m = S (plus n m)\n\n";
    let add_all = "addAll : (k : [] Nat) -> List Nat -> Nat\naddAll k xs =\n  let go : List Nat -> Nat\n      go Nil = Z\n      go (x :: rest) = plus k (plus x (go rest))\n  in go xs\n\n";
    let text = lowered(&format!("{nat}{add_all}addAll (S Z) (Z :: S Z :: Nil)\n"));
    // what it uses of where it was, `k`, is its first parameter
    has(&text, "def idyll:Main/addAll.go (k_N: ptr, x_N: ptr; k_N: ptr) =");
    // and every use passes it, the recursive one too
    has(&text, "idyll:Main/addAll.go(k_N, xs_N; k_N)");
    has(&text, "idyll:Main/addAll.go(k_N, rest_N;");
    assert!(!text.contains("not lowered"), "{text}");
}

#[test]
fn mutually_recursive_definitions_inside_another_pass_on_what_they_capture() {
    let nat = "data Nat = Z | S Nat\n\nuse Nat.*\n\n";
    let parity = "parity : (flip : [] Bool) -> Nat -> Bool\nparity flip n =\n  let even : Nat -> Bool\n      even Z = flip\n      even (S m) = odd m\n      odd : Nat -> Bool\n      odd Z = if flip then False else True\n      odd (S m) = even m\n  in even n\n\n";
    let text = lowered(&format!("{nat}{parity}parity True (S (S Z))\n"));
    // each uses `flip`, so each takes it, and passes it to the other
    has(&text, "def idyll:Main/parity.even (flip_N: bool, x_N: ptr; k_N: ptr) =");
    has(&text, "def idyll:Main/parity.odd (flip_N: bool, x_N: ptr; k_N: ptr) =");
    has(&text, "idyll:Main/parity.odd(flip_N, m_N; k_N)");
    has(&text, "idyll:Main/parity.even(flip_N, m_N; k_N)");
}

#[test]
fn a_lifted_definition_is_told_the_types_it_was_generic_in() {
    let rev = "rev : {a : Type} -> List a -> List a\nrev xs =\n  let go : List a -> List a -> List a\n      go Nil acc = acc\n      go (x :: rest) acc = go rest (x :: acc)\n  in go xs Nil\n\n";
    let text = lowered(&format!("{rev}rev (1 :: 2 :: Nil)\n"));
    // `a` is the enclosing function's, so its descriptor is passed; `xs`,
    // which only an erased argument mentions, is not
    has(&text, "def idyll:Main/rev.go <'a_N = d_a_N> (d_a_N: desc, x_N: ptr, acc_N: ptr; k_N: ptr) =");
    has(&text, "idyll:Main/rev.go(d_a_N, xs_N, v_N; k_N)");
    has(&text, "idyll:Main/rev.go(d_a_N, rest_N, v_N; k_N)");
}

#[test]
fn what_a_program_uses_of_std_is_lowered_with_it_under_stds_symbols() {
    let text = lowered("use Std.Collections.List (sum)

sum #[1, 2]
");
    has(&text, "def idyll:Std/Collections.List.sum (");
    // and nothing of it that the program does not use
    assert!(!text.contains("idyll:Std/Collections.List.map"), "{text}");
}

#[test]
fn abort_is_given_up_on_in_the_programs_own_code() {
    // no runtime operation aborts: a handler around the entry writes the
    // message to standard error and exits with status 1, as Idyll's world
    let text = lowered("main : () -> () ! Abort\nmain () = abort \"no\"\n");
    has(&text, "idyll:Prelude/Abort.abort(message_N: str; r_N: ptr, k_N: ptr) =>");
    has(&text, "perform idyll:Prelude/Console.putErr(line_N;");
    has(&text, "perform idyll:Prelude/Process.exit(1;");
    // and it is not one of the runtime's
    assert!(!text.contains("Abort.abort = "), "{text}");
}

#[test]
fn an_array_is_the_runtimes_array() {
    let text = lowered("arrayGet (arrayPush #[1; 2] 3) 0\n");
    has(&text, "<[1, 2] | μ̃ v_N: ptr.\n    prim arrayPush(v_N, 3; μ̃ v_N: ptr.\n      prim arrayGet(v_N, 0; μ̃ answer_N: i64.");
}

#[test]
fn listing_a_directory_is_the_runtimes_read_dir() {
    let text = lowered(
        "first : (r : [0..1] Result String (Array String)) -> String
first (Ok names) = arrayGetOr \"none\" names 0
first (Err e) = e

main : () -> () ! {Fs, Console}
main () = putStrLn (first (listDir \".\"))
",
    );
    has(&text, "Fs.readDir");
}

#[test]
fn every_program_in_the_corpora_lowers() {
    // or says why not, which is only ever something not lowered yet
    let corpora = [include_str!("corpus/handwritten.txt"), include_str!("corpus/tutorial.txt")];
    let (mut lowered_ok, mut unexpected) = (0, Vec::new());
    for corpus in corpora {
        for src in corpus.replace("\r\n", "\n").split("\n%%%\n") {
            let out = pipeline::compile(src, &Options { debug: false, style: OutputStyle::PLAIN, live: false });
            if out.has_errors() || out.checked.as_ref().and_then(|c| c.program.as_ref()).is_none() {
                continue;
            }
            let mut checked = out.checked.expect("checked");
            match idyllic_cut::lower::lower(&mut checked, "Main") {
                Ok(_) => lowered_ok += 1,
                Err(errors) => {
                    for e in errors {
                        if !e.contains("not lowered yet") && !e.contains("has no runtime operation") {
                            unexpected.push(format!("{e}\nin:\n{src}"));
                        }
                    }
                }
            }
        }
    }
    assert!(lowered_ok > 40, "only {lowered_ok} programs lowered");
    assert!(unexpected.is_empty(), "{}", unexpected.join("\n\n"));
}

#[test]
fn an_answer_is_written_out_as_the_repl_writes_it() {
    // the runtimes print only strings, so a program's value is shown in
    // Cut, as the REPL shows it, and answered as a string
    let text = lowered("Just (1, True)\n");
    has(&text, "answer str");
    has(&text, "def idyll:Main/\"#show Maybe (Int, Bool)\" (x_N: ptr; k_N: ptr) =");
    has(&text, "prim concatStrings([\"Just\", \" \", s_N]; k_N)");
    // a tuple brings its own parentheses, and an integer is the prelude's
    // `showInt`
    has(&text, "prim concatStrings([\"(\", s_N, \")\"]; k_N)");
    has(&text, "def idyll:Prelude/showInt (");
    // a field with fields of its own is parenthesised
    let text = lowered("Just (1 :: Nil)\n");
    has(&text, "def idyll:Main/\"#atom List Int\"");
    // an array, a string and a box
    let text = lowered("#[\"a\"; \"b\"]\n");
    has(&text, "def idyll:Main/\"#show Array String\"");
    has(&text, "prim concatStrings([\"\\\"\", e_N, \"\\\"\"]");
    // a string is escaped as the REPL escapes it, `\t` for a tab, byte by
    // byte, with the runtime's string primitives
    let text = lowered("\"a\\tb\"\n");
    has(&text, "def idyll:Main/\"#escape\" (s_N: str; k_N: ptr) =");
    has(&text, "prim stringByteAt(s_N, i_N;");
    has(&text, "<\"\\\\t\" | k_N>");
    // a function has nothing to show
    has(&lowered("\\(x : Int) -> x\n"), "answer none");
    // and a run of `main` answers nothing
    has(&lowered("main : () -> () ! Console\nmain () = putStrLn \"hi\"\n"), "answer none");
}

#[test]
fn an_effect_variable_is_declared_on_its_def() {
    let text = lowered("twice : (f : [2] (Int -> Int ! e)) -> Int -> Int ! e\ntwice f x = f (f x)\n\ntwice (\\(n : Int) -> n + 1) 0\n");
    has(&text, "def idyll:Main/twice <e_N: Effect> (f_N: ptr, x_N: i64; k_N: ptr) =");
    // one that may stand for an effect resumed more than once
    let text = lowered("again : {e : Effect []} -> (f : [2] (() -> Int ! e)) -> Int ! e\nagain f = f () + f ()\n\nagain (\\(u : ()) -> 1)\n");
    has(&text, "def idyll:Main/again <e_N: ManyEffect> (f_N: ptr; k_N: ptr) =");
}

#[test]
fn a_package_with_a_version_is_named_with_it() {
    // `idyll:Util@0.1.0/double`: the symbols a back end links by say which
    // version of a package they are from
    let mut source = pipeline::PackageSource::single("Util", "src/Lib.idl", "double : Int -> Int\ndouble n = n * 2\n\ndouble 21\n");
    source.version = Some("0.1.0".into());
    let out = pipeline::check_package_with(&source, pipeline::Deps::default(), &Options { debug: false, style: OutputStyle::PLAIN, live: false }, true);
    assert!(!out.has_errors(), "{:#?}", out.diagnostics);
    let mut checked = out.checked.expect("checked");
    let text = idyllic_cut::lower::lower(&mut checked, "Util@0.1.0").expect("lowers").to_string();
    has(&text, "def idyll:Util@0.1.0/double (");
    // the prelude has none
    has(&text, "idyll:Prelude/showInt");
}

#[test]
fn a_native_with_no_primitive_is_a_def_of_the_programs_own() {
    // generic in the element, as a Cut function is: a descriptor for it
    let text = lowered("arrayToList #[1; 2]\n");
    has(&text, "def idyll:Main/\"#arrayToList\" <'a_N = d_N> (d_N: desc, xs_N: ptr; k_N: ptr) =");
    has(&text, "idyll:Main/\"#arrayToList\"(desc(i64), v_N;");
    has(&text, "prim arrayGet(xs_N, i_N; μ̃ e_N: 'a_N.");
    // a list that is not written out is pushed, element by element
    let text = lowered("f : List Int -> Array Int\nf xs = arrayFromList xs\n\nf (1 :: Nil)\n");
    has(&text, "def idyll:Main/\"#arrayFromList\".go <'a_N = d_N> (d_N: desc, l_N: ptr, acc_N: ptr; k_N: ptr) =");
    // a mutable array is the runtime's, and a read gives it back with what
    // it read boxed
    let text = lowered("Std.Collections.MArray.freeze (Std.Collections.MArray.write (Std.Collections.MArray.new 2 0) 1 5)\n");
    has(&text, "def idyll:Main/\"#marrayNew\"");
    has(&text, "prim arraySet(");
}
