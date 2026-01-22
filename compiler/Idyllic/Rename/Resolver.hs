module Idyllic.Rename.Resolver where

import Control.Monad.State (MonadState (get, put), State, runState)
import Data.Text (Text)
import Idyllic.Rename.HIR
import Idyllic.Rename.Symbol (Symbol (Symbol))
import qualified Idyllic.Syn.AST as AST

-- newtype RenameError = REUnboundName Text deriving (Show, Eq)

-- data Env = Env
--   { nameMap :: [(Text, Int)],
--     nameCounter :: Int,
--     nodeIdCounter :: Int,
--     errors :: [RenameError]
--   }
--   deriving (Show, Eq)

-- defaultEnv :: Env
-- defaultEnv =
--   let initialNames = zip ops [0 ..]
--    in Env
--         { nameMap = initialNames,
--           nameCounter = length ops,
--           nodeIdCounter = 0,
--           errors = []
--         }
--   where
--     ops = ["+", "-", "*", "/", "%", "neg", "==", "!=", "<", "<=", ">", ">=", "&&", "||"]

-- find :: Text -> Rename (Maybe Int)
-- find name = do
--   env <- get
--   case lookup name (nameMap env) of
--     Just n -> pure (Just n)
--     Nothing -> do
--       put env {errors = REUnboundName name : errors env}
--       pure Nothing

-- freshName :: Text -> Rename Int
-- freshName name = do
--   env <- get
--   put env {nameMap = (name, nameCounter env) : nameMap env, nameCounter = nameCounter env + 1}
--   pure $ nameCounter env

-- freshNodeId :: Rename Int
-- freshNodeId = do
--   env <- get
--   put env {nodeIdCounter = nodeIdCounter env + 1}
--   pure $ nodeIdCounter env

-- type Rename a = State Env a

-- rename :: AST.Expr -> Either [RenameError] Expr
-- rename expr =
--   let (result, env) = runState (renameExpr expr) defaultEnv
--    in case errors env of
--         [] -> Right result
--         errs -> Left (reverse errs)

-- renameExpr :: AST.Expr -> Rename Expr
-- renameExpr expr = go $ synNodeKind expr
--   where
--     go :: AST.ExprKind -> Rename Expr
--     go (AST.Lit (AST.LitInt n)) = do
--       nodeId <- freshNodeId
--       pure $ HirNode nodeId (ExprLit (LitInt n)) (synNodeSpan expr)
--     go (AST.Lit (AST.LitBool b)) = do
--       nodeId <- freshNodeId
--       pure $ HirNode nodeId (ExprLit (LitBool b)) (synNodeSpan expr)
--     go (AST.Lit (AST.LitString s)) = do
--       nodeId <- freshNodeId
--       pure $ HirNode nodeId (ExprLit (LitString s)) (synNodeSpan expr)
--     go (AST.Var x) = do
--       nodeId <- freshNodeId
--       var <- find $ synNodeKind x
--       let sym = Symbol <$> var <*> pure (synNodeSpan x)
--       let m = maybe ExprError ExprVar sym
--       pure $ HirNode nodeId m (synNodeSpan expr)
--     go (AST.Let bind body) = do
--       nodeId <- freshNodeId
--       (bind', body') <- renameBind bind body
--       pure $ HirNode nodeId (ExprLet bind' body') (synNodeSpan expr)
--       where
--         renameBind :: AST.Bind -> AST.Expr -> Rename (Bind, Expr)
--         renameBind (AST.BindName x e) b = do
--           x' <- Symbol <$> freshName (synNodeKind x) <*> pure (synNodeSpan x) -- we add the name to the env first since laziness means it can be recursive
--           e' <- renameExpr e
--           body' <- renameExpr b
--           pure (BindName x' e', body')
--         renameBind (AST.BindFun f args e) b = do
--           f' <- Symbol <$> freshName (synNodeKind f) <*> pure (synNodeSpan f)
--           args' <- mapM (\p -> Symbol <$> freshName (synNodeKind p) <*> pure (synNodeSpan p)) args
--           e' <- renameExpr e
--           body' <- renameExpr b
--           pure (BindFun f' args' e', body')
--     go (AST.If cond thenBr elseBr) = do
--       nodeId <- freshNodeId
--       cond' <- renameExpr cond
--       thenBr' <- renameExpr thenBr
--       elseBr' <- renameExpr elseBr
--       pure $ HirNode nodeId (ExprIf cond' thenBr' elseBr') (synNodeSpan expr)
--     go (AST.Lam params body) = do
--       nodeId <- freshNodeId
--       params' <- mapM (\p -> Symbol <$> freshName (synNodeKind p) <*> pure (synNodeSpan p)) params
--       body' <- renameExpr body
--       pure $ HirNode nodeId (ExprLam params' body') (synNodeSpan expr)
--     go (AST.App f args) = do
--       nodeId <- freshNodeId
--       f' <- renameExpr f
--       args' <- mapM renameExpr args
--       pure $ HirNode nodeId (ExprApp f' args') (synNodeSpan expr)
--     go (AST.Infix l op r) = do
--       nodeId <- freshNodeId
--       l' <- renameExpr l
--       opId <- freshNodeId
--       r' <- renameExpr r
--       opSym <- find (synNodeKind op)
--       let opNode = case opSym of
--             Just s -> ExprVar (Symbol s (synNodeSpan op))
--             Nothing -> ExprError
--       pure $ HirNode nodeId (ExprApp (HirNode opId opNode (synNodeSpan op)) [l', r']) (synNodeSpan expr)
--     go (AST.Neg e) = do
--       nodeId <- freshNodeId
--       opId <- freshNodeId
--       e' <- renameExpr e
--       opSym <- find "neg"
--       let opNode = case opSym of
--             Just s -> ExprVar (Symbol s (synNodeSpan expr))
--             Nothing -> ExprError
--       pure $ HirNode nodeId (ExprApp (HirNode opId opNode (synNodeSpan expr)) [e']) (synNodeSpan expr)