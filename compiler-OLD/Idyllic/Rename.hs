module Idyllic.Rename where

import Control.Monad.State
import Data.Map (Map)
import Data.Map qualified as Map
import Data.Text (Text)
import Idyllic.AST qualified as AST
import Idyllic.HIR
import Idyllic.Types
import Idyllic.Utils

defaultEnv :: NameEnv
defaultEnv =
  NameEnv
    { nameMap = initialNames,
      nameCounter = length ops,
      nodeIdCounter = 0,
      errors = []
    }
  where
    ops = ["+", "-", "*", "/", "%", "neg", "==", "!=", "<", "<=", ">", ">=", "&&", "||", "print", "println", "error"]
    initialNames = Map.fromList $ zip ops [0 ..]

type Rename a = State NameEnv a

find :: Text -> Rename (Maybe Int)
find name = do
  env <- get
  case Map.lookup name (nameMap env) of
    Just n -> pure (Just n)
    Nothing -> do
      put env {errors = ResUnboundName name : errors env}
      pure Nothing

freshName :: Text -> Rename Int
freshName name = do
  env <- get
  put env {nameMap = Map.insert name (nameCounter env) (nameMap env), nameCounter = nameCounter env + 1}
  pure $ nameCounter env

freshNodeId :: Rename Int
freshNodeId = do
  env <- get
  put env {nodeIdCounter = nodeIdCounter env + 1}
  pure $ nodeIdCounter env

-- runRename :: AST.Prog -> (Prog, [RenameError])
-- runRename prog = let (p, env) = runState (renameModule prog) defaultEnv in (p, errors env)

-- renameModule :: AST.Prog -> Rename Prog
-- renameModule (Located (AST.Module name decls) s) = do
--   decls' <- mapM renameBind decls
--   pure $ Located (Module name decls') s

-- renameExpr :: AST.LExpr -> Rename LExpr
-- renameExpr (Located (AST.ExprLit l) s) = pure $ Located (ExprLit (mapLit l)) s
-- renameExpr (Located (AST.ExprVar x) s) = do
--   varId <- find (unLoc x)
--   case varId of
--     Just n -> pure $ Located (ExprVar (Located (n, unLoc x) s)) s
--     Nothing -> pure $ Located (ExprVar (Located (-1, unLoc x) s)) s
-- renameExpr (Located (AST.ExprApp f args) s) = do
--   f' <- renameExpr f
--   args' <- mapM renameExpr args
--   pure $ Located (ExprApp f' args') s
-- renameExpr (Located (AST.ExprLam args body) s) = do
--   args' <- mapM (\p -> do pId <- freshName (unLoc p); pure $ Located (pId, unLoc p) s) args
--   body' <- renameExpr body
--   pure $ Located (ExprLam args' body') s
-- renameExpr (Located (AST.ExprLet binds body) s) = do
--   binds' <- mapM renameBind binds
--   body' <- renameExpr body
--   pure $ Located (ExprLet binds' body') s
-- renameExpr (Located AST.ExprUnit s) = pure $ Located ExprUnit s

-- renameBind :: AST.LBind -> Rename LBind
-- renameBind (Located (AST.BindName x e) s) = do
--   xId <- freshName (unLoc x)
--   e' <- renameExpr e
--   pure $ Located (BindName (Located (xId, unLoc x) s) e') s
-- renameBind (Located (AST.BindFun f args e) s) = do
--   fId <- freshName (unLoc f)
--   args' <- mapM (\p -> do pId <- freshName (unLoc p); pure $ Located (pId, unLoc p) s) args
--   e' <- renameExpr e
--   pure $ Located (BindFun (Located (fId, unLoc f) s) args' e') s

-- mapLit :: AST.Lit -> Lit
-- mapLit (AST.LitInt n) = LitInt n
-- mapLit (AST.LitBool b) = LitBool b
-- mapLit (AST.LitString str) = LitString str
