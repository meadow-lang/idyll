module Idyllic.Syn.Parser where

import Control.Monad.Combinators.Expr (Operator (..), makeExprParser)
import Data.Text (Text, pack, unpack)
import Data.Void
import Idyllic.Syn.AST
import Idyllic.Syn.Token (Token (..))
import Idyllic.Utils.Span (Span (..))
import Text.Megaparsec (Parsec)

type Parser = Parsec Void [Token]
