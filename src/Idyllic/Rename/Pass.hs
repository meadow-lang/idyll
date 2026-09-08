-- | Name resolution: surface "Idyllic.Syntax.AST" to "Idyllic.Syntax.HIR".
--
--   Responsibilities:
--
--     * Resolve every 'AST.TermVar' to an 'HIR.Ident' with a scope-unique
--       integer; report 'UnboundName' for anything not in scope.
--
--     * Introduce fresh uniques for every binder (lambda / Pi / @let@), with
--       the usual lexical scoping (a Pi/lambda domain does not see the name
--       it binds; later telescope entries do; @let@ groups are recursive).
--
--     * Allocate a fresh 'HIR.NodeId' for every 'HIR.Term' and 'HIR.Bind',
--       recording its source 'Span' in the 'HIR.SpanTable' side table.
--
--   @let@ / top-level groups are emitted as one conservative 'HIR.Rec'
--   group; "Idyllic.Scc" refines that into dependency-ordered SCCs.
module Idyllic.Rename.Pass
  ( RenameResult (..),
    renameModule,
    renameTermTop,
  )
where

import Control.Monad (foldM, forM, zipWithM)
import Control.Monad.State.Strict
import Data.IntMap.Strict qualified as IntMap
import Data.Map.Strict qualified as Map
import Data.Text (Text)
import Idyllic.Rename.Env
import Idyllic.Syntax.AST qualified as AST
import Idyllic.Syntax.HIR qualified as H
import Idyllic.Utils (Located (..), Span (..))

-- ---------------------------------------------------------------------------
-- Entry points
-- ---------------------------------------------------------------------------

data RenameResult = RenameResult
  { rrModule :: H.Module,
    rrSpans :: H.SpanTable,
    rrErrors :: [RenameError]
  }
  deriving (Show, Eq)

-- | Rename a whole program.  A top-level @let ... in body@ is treated as a
--   module: its bindings become the top-level declarations and @body@ the
--   trailing entry expression.  Any other term becomes a module with no
--   declarations and that term as the body.
renameModule :: AST.LTerm -> RenameResult
renameModule prog = finish $ runState go emptyRenameEnv
  where
    go = case unLoc prog of
      AST.TermLet binds body -> do
        bs <- renameRecBinds binds
        body' <- renameTerm body
        pure (H.Module [H.Rec bs] body')
      _ -> H.Module [] <$> renameTerm prog

-- | Rename a bare term (no module peeling); handy for the REPL.
renameTermTop :: AST.LTerm -> RenameResult
renameTermTop t = finish $ runState (H.Module [] <$> renameTerm t) emptyRenameEnv

finish :: (H.Module, RenameEnv) -> RenameResult
finish (m, env) =
  RenameResult
    { rrModule = m,
      rrSpans = reSpans env,
      rrErrors = reverse (reErrors env)
    }

-- ---------------------------------------------------------------------------
-- Monad helpers
-- ---------------------------------------------------------------------------

type Rename a = State RenameEnv a

freshUnique :: Rename Int
freshUnique = state $ \env ->
  (reNextUnique env, env {reNextUnique = reNextUnique env + 1})

-- | Allocate a node id and record its span in the side table.
freshNode :: Span -> Rename H.NodeId
freshNode sp = state $ \env ->
  let nid = reNextNode env
   in ( H.NodeId nid,
        env
          { reNextNode = nid + 1,
            reSpans = IntMap.insert nid sp (reSpans env)
          }
      )

node :: Span -> H.TermKind -> Rename H.Term
node sp k = do
  nid <- freshNode sp
  pure (H.Term nid k)

logError :: RenameError -> Rename ()
logError e = modify' $ \env -> env {reErrors = e : reErrors env}

-- | Push a new innermost scope, run @m@, pop it.
scoped :: Rename a -> Rename a
scoped m = do
  modify' $ \env -> env {reScopes = Map.empty : reScopes env}
  x <- m
  modify' $ \env -> env {reScopes = drop 1 (reScopes env)}
  pure x

-- | Bind @name@ in the innermost scope, returning its fresh 'H.Ident'.
--   Records a 'DuplicateBinding' error if the name is already bound in that
--   same scope.
bindName :: Located Text -> Rename H.Ident
bindName (Located name sp) = do
  u <- freshUnique
  scopes <- gets reScopes
  case scopes of
    [] -> error "Rename: empty scope stack"
    (cur : rest) -> do
      case Map.lookup name cur of
        Just _prev ->
          -- We don't track prior spans per scope; report with the same span
          -- twice rather than growing the env.  Good enough for the demo.
          logError (DuplicateBinding name sp sp)
        Nothing -> pure ()
      -- 'modify'' (not 'put' on a stale snapshot) so this doesn't clobber
      -- the error 'logError' may have just recorded.
      modify' $ \env -> env {reScopes = Map.insert name u cur : rest}
      pure (H.Ident u name)

-- | Resolve a use of @name@.  On failure, log 'UnboundName' and return an
--   'H.Ident' with a sentinel unique of @-1@ so downstream passes still get
--   a well-formed tree.
useName :: Located Text -> Rename H.Ident
useName (Located name sp) = do
  env <- get
  case lookupScopes name (reScopes env) of
    Just u -> pure (H.Ident u name)
    Nothing -> do
      logError (UnboundName name sp)
      pure (H.Ident (-1) name)

lookupScopes :: Text -> [Scope] -> Maybe Int
lookupScopes _ [] = Nothing
lookupScopes name (s : ss) = case Map.lookup name s of
  Just u -> Just u
  Nothing -> lookupScopes name ss

-- ---------------------------------------------------------------------------
-- Terms
-- ---------------------------------------------------------------------------

renameTerm :: AST.LTerm -> Rename H.Term
renameTerm (Located t sp) = case t of
  AST.TermType -> node sp H.HType
  AST.TermLit l -> node sp (H.HLit l)
  AST.TermHole -> node sp H.HHole
  AST.TermVar x -> do
    i <- useName x
    node sp (H.HVar i)
  AST.TermApp f a -> do
    f' <- renameTerm f
    a' <- renameTerm a
    node sp (H.HApp f' a')
  AST.TermAnn e a -> do
    e' <- renameTerm e
    a' <- renameTerm a
    node sp (H.HAnn e' a')
  AST.TermPi mb dom cod -> do
    -- the domain cannot see the bound name
    dom' <- renameTerm dom
    scoped $ do
      mb' <- traverse bindName mb
      cod' <- renameTerm cod
      node sp (H.HPi mb' dom' cod')
  AST.TermLam (Located (AST.Param n mty) _) body -> do
    -- the annotation cannot see the bound name
    mty' <- traverse renameTerm mty
    scoped $ do
      n' <- bindName n
      body' <- renameTerm body
      node sp (H.HLam n' mty' body')
  AST.TermLet binds body ->
    scoped $ do
      bs <- renameRecBinds binds
      body' <- renameTerm body
      node sp (H.HLet [H.Rec bs] body')

-- ---------------------------------------------------------------------------
-- Bindings
-- ---------------------------------------------------------------------------

-- | Rename a recursive group of bindings: every bound name is in scope for
--   every right-hand side (and for the body, handled by the caller).
renameRecBinds :: [AST.LBind] -> Rename [H.Bind]
renameRecBinds lbinds = do
  -- Pass 1: introduce all names so the group is mutually recursive.
  idents <- forM lbinds $ \(Located b _) -> bindName (bindNameOf b)
  -- Pass 2: rename each right-hand side.
  zipWithM renameOneBind lbinds idents

bindNameOf :: AST.Bind -> Located Text
bindNameOf (AST.BindName n _ _) = n
bindNameOf (AST.BindFun n _ _ _) = n

renameOneBind :: AST.LBind -> H.Ident -> Rename H.Bind
renameOneBind (Located b sp) ident = do
  nid <- freshNode sp
  case b of
    AST.BindName _ mty rhs -> do
      mty' <- traverse renameTerm mty
      rhs' <- renameTerm rhs
      pure (H.Bind nid ident mty' rhs')
    AST.BindFun _ params mty rhs ->
      scoped $ do
        -- params scope over each other's later annotations, the result-type
        -- annotation, and the body
        ps <- renameParams params
        body0 <- renameTerm rhs
        body1 <- case mty of
          Nothing -> pure body0
          Just ty -> do
            ty' <- renameTerm ty
            annNode (spanOf rhs <> spanOf ty) body0 ty'
        lam <- foldM (wrapLam sp) body1 (reverse ps)
        pure (H.Bind nid ident Nothing lam)
  where
    spanOf = locSpan
    annNode s e a = do
      n <- freshNode s
      pure (H.Term n (H.HAnn e a))
    wrapLam s acc (pn, pty) = do
      n <- freshNode s
      pure (H.Term n (H.HLam pn pty acc))

-- | Rename a left-to-right telescope of binders, each entry visible to the
--   annotations of the entries that follow it.
renameParams :: [AST.LParam] -> Rename [(H.Ident, Maybe H.Term)]
renameParams [] = pure []
renameParams (Located (AST.Param n mty) _ : rest) = do
  mty' <- traverse renameTerm mty
  n' <- bindName n
  ((n', mty') :) <$> renameParams rest
