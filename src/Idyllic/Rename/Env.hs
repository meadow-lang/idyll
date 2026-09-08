-- | State and errors for the name-resolution pass ("Idyllic.Rename.Pass").
module Idyllic.Rename.Env
  ( RenameEnv (..),
    RenameError (..),
    Scope,
    emptyRenameEnv,
    renameErrorReport,
  )
where

import Data.IntMap.Strict (IntMap)
import Data.IntMap.Strict qualified as IntMap
import Data.Map.Strict (Map)
import Data.Map.Strict qualified as Map
import Data.Text (Text)
import Error.Diagnose (Marker (..), Position (..), Report, err)
import Idyllic.Builtins
import Idyllic.Utils (LineIndex, Span (..), offsetToLineCol)

-- | One lexical scope: surface spelling to the unique it was bound with, and
--   where it was bound (so a later duplicate can point back at the original).
type Scope = Map Text (Int, Span)

data RenameEnv = RenameEnv
  { -- | Active scopes, innermost first.  Always non-empty (the last element
    --   is the global/module scope).
    reScopes :: [Scope],
    -- | Next name unique to hand out.
    reNextUnique :: Int,
    -- | Next HIR 'Idyllic.Syntax.HIR.NodeId' to hand out.
    reNextNode :: Int,
    -- | Node id (as 'Int') to source span, accumulated as nodes are built.
    reSpans :: IntMap Span,
    -- | Errors, newest first.
    reErrors :: [RenameError]
  }
  deriving (Show, Eq)

data RenameError
  = -- | A name with no binding in scope.
    UnboundName Text Span
  | -- | Two bindings of the same name in one group / scope.
    DuplicateBinding Text Span Span
  deriving (Show, Eq)

-- | The starting environment: a single global scope pre-populated with the
--   primitives from "Idyllic.Builtins", holding the uniques those builtins
--   reserve.  The type checker seeds its initial context the same way, which
--   is how the two passes agree on what @Int@ means.
emptyRenameEnv :: RenameEnv
emptyRenameEnv =
  RenameEnv
    { reScopes = [Map.fromList [(builtinName b, (builtinUnique b, Span 0 0)) | b <- allBuiltins]],
      reNextUnique = builtinCount,
      reNextNode = 0,
      reSpans = IntMap.empty,
      reErrors = []
    }

renameErrorReport :: FilePath -> LineIndex -> RenameError -> Report Text
renameErrorReport file li e = case e of
  UnboundName name sp ->
    single sp ("name not in scope: " <> name) "used here"
  DuplicateBinding name earlier later ->
    err
      Nothing
      ("duplicate binding of " <> name)
      [ (posOf earlier, Where "first bound here"),
        (posOf later, This "bound again here")
      ]
      []
  where
    single sp msg lbl = err Nothing msg [(posOf sp, This lbl)] []
    posOf (Span s eoff) =
      let (l1, c1) = offsetToLineCol li s
          (l2, c2) = offsetToLineCol li eoff
       in Position (l1, c1) (l2, c2) file
