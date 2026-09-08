-- | Core: the elaborated, fully explicit term language the type checker works
--   with.
--
--   Unlike "Idyllic.Syntax.HIR", Core uses de Bruijn /indices/ and carries no
--   node ids — it is produced by elaboration and by quoting values back from
--   the semantic domain ("Idyllic.TypeCheck.Value").  Because binders are
--   nameless, structural equality on 'Core' /is/ alpha-equality (the 'Text'
--   in 'CVar' \/ 'CLam' \/ 'CPi' is only a display hint and is ignored by
--   '(==)').
--
--   Note that Core terms are only meaningful relative to the context they
--   were elaborated in: a free index refers to a context entry.  The type
--   checker never moves a Core term out of its context.
module Idyllic.TypeCheck.Core
  ( Core (..),
    Idx,
    Lvl,
    MetaVar,
    litBuiltin,
    showCore,
  )
where

import Data.Text (Text)
import Data.Text qualified as T
import Idyllic.Builtins
import Idyllic.Syntax.AST (Lit (..))

-- | de Bruijn index: 0 is the innermost binder.  Used in 'Core'.
type Idx = Int

-- | de Bruijn level: 0 is the outermost binder.  Used in values.
type Lvl = Int

-- | Identifies an entry in the metacontext (see "Idyllic.TypeCheck.Monad").
type MetaVar = Int

data Core
  = -- | The sort.  This calculus is @Type : Type@; see "Idyllic.TypeCheck".
    CType
  | -- | A primitive type constant (@Int@, @Bool@, ...).
    CLitTy Builtin
  | CLit Lit
  | -- | A variable, by de Bruijn index, plus a name hint for display.
    CVar Idx Text
  | -- | A metavariable: a not-yet-known term, to be solved by unification.
    --   Metas are always /closed/; the elaborator inserts them applied to
    --   the bound variables in scope, so this constructor never captures.
    CMeta MetaVar
  | -- | @(x : a) -> b@; the hint is @\"_\"@ for a non-dependent arrow.
    CPi Text Core Core
  | CLam Text Core
  | CApp Core Core
  | -- | Stands in for a subterm that failed to elaborate, so that one error
    --   does not cascade into a hundred.
    CError
  deriving (Show)

-- | Alpha-equality: name hints are display-only and do not participate.
--   (The real equality used by the checker is 'Idyllic.TypeCheck.Value.conv',
--   which is up to beta/eta; this instance is for tests and for cheap
--   syntactic checks.)
instance Eq Core where
  CType == CType = True
  CLitTy a == CLitTy b = a == b
  CLit a == CLit b = a == b
  CVar i _ == CVar j _ = i == j
  CMeta a == CMeta b = a == b
  CPi _ a b == CPi _ a' b' = a == a' && b == b'
  CLam _ b == CLam _ b' = b == b'
  CApp f a == CApp g b = f == g && a == b
  CError == CError = True
  _ == _ = False

-- | The primitive type of a literal.
litBuiltin :: Lit -> Builtin
litBuiltin = \case
  LitUnit -> BuiltinUnit
  LitInt _ -> BuiltinInt
  LitBool _ -> BuiltinBool
  LitString _ -> BuiltinString

-- | Render a Core term for error messages.  Binders are shown using their
--   name hints, so shadowed names can look ambiguous; that is a display
--   nicety only, the term itself is nameless.
showCore :: Core -> Text
showCore = go 0
  where
    go :: Int -> Core -> Text
    go _ CType = "Type"
    go _ (CLitTy b) = builtinName b
    go _ (CLit l) = showLit l
    go _ (CVar _ n) = n
    go _ (CMeta m) = "?" <> T.pack (show m)
    go _ CError = "?!"
    go p (CApp f a) = paren (p > 10) (go 10 f <> " " <> go 11 a)
    go p (CLam x b) = paren (p > 0) ("\\" <> x <> " -> " <> go 0 b)
    go p (CPi x a b)
      | x == "_" = paren (p > 0) (go 1 a <> " -> " <> go 0 b)
      | otherwise = paren (p > 0) ("(" <> x <> " : " <> go 0 a <> ") -> " <> go 0 b)

    paren True s = "(" <> s <> ")"
    paren False s = s

    showLit = \case
      LitUnit -> "()"
      LitInt n -> T.pack (show n)
      LitBool True -> "True"
      LitBool False -> "False"
      LitString s -> T.pack (show s)
