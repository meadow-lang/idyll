-- | Primitive constants that are in scope before any user code.
--
--   Their 'builtinUnique's deliberately occupy the first name uniques, so the
--   renamer seeds its global scope with them and starts handing out fresh
--   uniques at 'builtinCount'.  The type checker seeds its initial context
--   with the same uniques, which is what lets the two passes agree without
--   threading a table between them.
module Idyllic.Builtins
  ( Builtin (..),
    allBuiltins,
    builtinName,
    builtinUnique,
    builtinCount,
  )
where

import Data.Text (Text)

-- | The primitive type constants.  Each is a closed type of sort @Type@.
data Builtin
  = BuiltinUnit
  | BuiltinInt
  | BuiltinBool
  | BuiltinString
  deriving (Show, Eq, Ord, Enum, Bounded)

allBuiltins :: [Builtin]
allBuiltins = [minBound .. maxBound]

builtinName :: Builtin -> Text
builtinName = \case
  BuiltinUnit -> "Unit"
  BuiltinInt -> "Int"
  BuiltinBool -> "Bool"
  BuiltinString -> "String"

-- | The name unique reserved for this builtin.
builtinUnique :: Builtin -> Int
builtinUnique = fromEnum

-- | The first unique available for user-defined names.
builtinCount :: Int
builtinCount = length allBuiltins
