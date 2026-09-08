-- | Strongly-connected-components pass over renamed bindings.
--
--   Runs after name resolution ("Idyllic.Rename.Pass") and before type
--   checking.  The renamer emits every @let@ / top-level group as a single
--   conservative 'H.Rec'; this pass rebuilds those groups from the actual
--   dependency graph:
--
--     * bindings that do not (transitively, within the group) refer to
--       themselves become 'H.NonRec';
--
--     * mutually-recursive bindings are collected into one 'H.Rec';
--
--     * the resulting groups are ordered so that each group only refers to
--       earlier groups (topological order) — this is the \"retopologize\"
--       step.
--
--   Node ids are preserved, so the span / type side tables stay valid.
module Idyllic.Scc
  ( sccModule,
    sccTerm,
    sccBinds,
    freeUniques,
  )
where

import Data.Graph (SCC (..), stronglyConnComp)
import Data.Set (Set)
import Data.Set qualified as Set
import Idyllic.Syntax.HIR qualified as H

-- | Retopologize a whole module: its top-level groups first, then any
--   nested @let@s in the trailing body.
sccModule :: H.Module -> H.Module
sccModule (H.Module groups body) =
  H.Module (sccBinds (concatMap H.bindGroupBinds groups)) (sccTerm body)

-- | Rewrite every @let@ in a term so its groups are dependency-ordered
--   SCCs, recursing into all subterms.
sccTerm :: H.Term -> H.Term
sccTerm (H.Term nid k) = H.Term nid (sccKind k)

sccKind :: H.TermKind -> H.TermKind
sccKind k = case k of
  H.HType -> k
  H.HHole -> k
  H.HLit _ -> k
  H.HVar _ -> k
  H.HApp f a -> H.HApp (sccTerm f) (sccTerm a)
  H.HAnn e a -> H.HAnn (sccTerm e) (sccTerm a)
  H.HPi mb dom cod -> H.HPi mb (sccTerm dom) (sccTerm cod)
  H.HLam x mty body -> H.HLam x (fmap sccTerm mty) (sccTerm body)
  H.HLet groups body ->
    H.HLet
      (sccBinds (map descend (concatMap H.bindGroupBinds groups)))
      (sccTerm body)
  where
    descend b = b {H.bindType = fmap sccTerm (H.bindType b), H.bindRhs = sccTerm (H.bindRhs b)}

-- | Split a mutually-recursive bind list into dependency-ordered groups.
--
--   'stronglyConnComp' returns the SCCs in reverse topological order (a
--   component appears before the components that depend on it), which is the
--   order we want for evaluation / type checking.
sccBinds :: [H.Bind] -> [H.BindGroup]
sccBinds binds = map toGroup (stronglyConnComp nodes)
  where
    boundHere :: Set Int
    boundHere = Set.fromList (map (H.idUnique . H.bindName) binds)

    nodes =
      [ (b, H.idUnique (H.bindName b), Set.toList (Set.intersection boundHere (freeUniquesBind b)))
        | b <- binds
      ]

    toGroup (AcyclicSCC b) = H.NonRec b
    toGroup (CyclicSCC bs) = H.Rec bs

-- | Free name uniques of a binding: those of its optional type annotation
--   and its right-hand side.  The binding's own name counts as free if the
--   rhs mentions it, which is exactly what makes direct recursion show up as
--   a self-edge.
freeUniquesBind :: H.Bind -> Set Int
freeUniquesBind b = foldMap freeUniques (H.bindType b) <> freeUniques (H.bindRhs b)

-- | Free name uniques of a term (uses of names not bound within it).
freeUniques :: H.Term -> Set Int
freeUniques (H.Term _ k) = case k of
  H.HType -> Set.empty
  H.HHole -> Set.empty
  H.HLit _ -> Set.empty
  H.HVar i -> Set.singleton (H.idUnique i)
  H.HApp f a -> freeUniques f <> freeUniques a
  H.HAnn e a -> freeUniques e <> freeUniques a
  H.HPi mb dom cod ->
    freeUniques dom <> maybe id (Set.delete . H.idUnique) mb (freeUniques cod)
  H.HLam x mty body ->
    foldMap freeUniques mty <> Set.delete (H.idUnique x) (freeUniques body)
  H.HLet groups body ->
    let bs = concatMap H.bindGroupBinds groups
        bound = Set.fromList (map (H.idUnique . H.bindName) bs)
        used = foldMap freeUniquesBind bs <> freeUniques body
     in used Set.\\ bound
