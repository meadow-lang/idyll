-- | The High-level IR: the surface AST after name resolution.
--
--   Differences from "Idyllic.Syntax.AST":
--
--     * Every name is an 'Ident' carrying a scope-unique 'Int', so binders
--       and their uses can be compared by identity rather than spelling.
--
--     * Every 'Term' and 'Bind' carries a 'NodeId' (a fresh 'Int').  Spans,
--       and later inferred types, live in side tables ('SpanTable',
--       @TypeTable@) keyed by that id rather than inline on the node.
--
--     * Recursive @let@ / top-level groups are represented as ordered
--       'BindGroup's.  The renamer emits one conservative recursive group;
--       "Idyllic.Scc" then splits it into strongly-connected components and
--       reorders them into dependency order.
module Idyllic.Syntax.HIR
  ( Ident (..),
    NodeId (..),
    Term (..),
    TermKind (..),
    Bind (..),
    BindGroup (..),
    bindGroupBinds,
    bindGroupBindNames,
    isRecursive,
    Module (..),
    SpanTable,
    Lit (..),
  )
where

import Data.IntMap.Strict (IntMap)
import Data.Text (Text)
import Idyllic.Syntax.AST (Lit (..))
import Idyllic.Utils (Span)

-- | A resolved name: a scope-unique integer plus the original spelling
--   (kept only for display / diagnostics).  Two 'Ident's denote the same
--   entity iff their 'idUnique's are equal.
data Ident = Ident
  { idUnique :: Int,
    idText :: Text
  }
  deriving (Show, Eq, Ord)

-- | The identity of an HIR node.  Freshly allocated per node; used to key
--   the side tables.
newtype NodeId = NodeId {unNodeId :: Int}
  deriving (Show, Eq, Ord)

-- | An HIR term: an identity plus its shape.
data Term = Term
  { termId :: NodeId,
    termKind :: TermKind
  }
  deriving (Show, Eq)

data TermKind
  = -- | The sort @Type@.
    HType
  | HVar Ident
  | HLit Lit
  | -- | Dependent function type; @Just x@ for @(x : a) -> b@, @Nothing@ for
    --   the plain arrow @a -> b@.
    HPi (Maybe Ident) Term Term
  | -- | @\\x -> e@ or @\\(x : a) -> e@.
    HLam Ident (Maybe Term) Term
  | HApp Term Term
  | -- | @e : a@.
    HAnn Term Term
  | -- | @_@: becomes a fresh metavariable during type checking.
    HHole
  | -- | @let@ with dependency-ordered groups.
    HLet [BindGroup] Term
  deriving (Show, Eq)

-- | A single binding, @x [: a] = e@.  Function sugar @f p1 p2 = e@ is
--   desugared by the renamer into @f = \\p1 p2 -> e@.
data Bind = Bind
  { bindId :: NodeId,
    bindName :: Ident,
    bindType :: Maybe Term,
    bindRhs :: Term
  }
  deriving (Show, Eq)

-- | A set of bindings that must be handled together.  Emitted by the SCC
--   pass: 'NonRec' for a binding that does not refer to itself or its
--   group, 'Rec' for a strongly-connected (mutually recursive) set.
data BindGroup
  = NonRec Bind
  | Rec [Bind]
  deriving (Show, Eq)

bindGroupBinds :: BindGroup -> [Bind]
bindGroupBinds (NonRec b) = [b]
bindGroupBinds (Rec bs) = bs

bindGroupBindNames :: BindGroup -> [Ident]
bindGroupBindNames = map bindName . bindGroupBinds

isRecursive :: BindGroup -> Bool
isRecursive (NonRec _) = False
isRecursive (Rec _) = True

-- | A renamed program: top-level bind groups (in dependency order after the
--   SCC pass) followed by a trailing expression (the REPL / demo entry
--   point; @HLit LitUnit@ when there is none).
data Module = Module
  { modGroups :: [BindGroup],
    modBody :: Term
  }
  deriving (Show, Eq)

-- | Side table: 'NodeId' (as its underlying 'Int') to source 'Span'.
type SpanTable = IntMap Span
