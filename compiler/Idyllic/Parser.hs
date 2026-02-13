-- module Idyllic.Syn.Parser (parseFile, parseInteractive) where
module Idyllic.Parser where

import Control.Monad (void)
import Control.Monad.Combinators.Expr
import Data.Functor (($>))
import Data.List (unsnoc)
import qualified Data.List.NonEmpty as NonEmpty
import Data.Maybe (fromMaybe, isJust)
import qualified Data.Set as Set
import Data.Text (Text, pack, unpack)
import Data.Void
import Error.Diagnose.Compat.Megaparsec (HasHints (..))
import Idyllic.AST
import Idyllic.Effect
import Idyllic.Token
import Idyllic.Utils
import Text.Megaparsec hiding (Token)

type Parser = Parsec Void [LToken]

located :: Parser a -> Parser (Located a)
located p = do
  startInp <- getInput
  x <- p
  endInp <- getInput
  case (startInp, endInp) of
    ([], _) -> fail "empty input"
    (t : _, t' : _) -> pure $ Located x (locSpan t <> locSpan t')
    (t : ts, []) -> case unsnoc ts of
      Nothing -> pure $ Located x (locSpan t)
      Just (_, eoi) -> pure $ Located x (locSpan t <> locSpan eoi)

tokenP :: Token -> Parser Token
tokenP t = token (\(Located lt _) -> if t == lt then Just t else Nothing) Set.empty

int :: Parser (Located Integer)
int = token (\case (Located (TokenInt n) s) -> Just (Located n s); _ -> Nothing) Set.empty

bool :: Parser (Located Bool)
bool = token test Set.empty
  where
    test (Located (TokenUppercaseIdent name) s) =
      case name of
        "True" -> Just (Located True s)
        "False" -> Just (Located False s)
        _ -> Nothing
    test _ = Nothing

string :: Parser (Located Text)
string = token (\case (Located (TokenString str) s) -> Just (Located str s); _ -> Nothing) Set.empty

unit :: Parser ()
unit = tokenP TokenLParen *> tokenP TokenRParen $> ()

lit :: Parser (Located Lit)
lit =
  choice
    [ fmap Int <$> int,
      fmap Bool <$> bool,
      fmap String <$> string
    ]

kw :: Text -> Parser LToken
kw target = token test expectedSet
  where
    test (Located (TokenLowercaseIdent name) s)
      | name == target = Just (Located (TokenLowercaseIdent name) s)
    test _ = Nothing

    -- This ensures the error message says: "expecting 'fun'"
    expectedSet = Set.singleton (Label (NonEmpty.fromList $ unpack target))

lowerIdent :: Parser Ident
lowerIdent = token (\case (Located (TokenLowercaseIdent name) s) -> Just (Located name s); _ -> Nothing) Set.empty

upperIdent :: Parser Ident
upperIdent = token (\case (Located (TokenUppercaseIdent name) s) -> Just (Located name s); _ -> Nothing) Set.empty

parens :: Parser a -> Parser a
parens = between (tokenP TokenLParen) (tokenP TokenRParen)

brackets :: Parser a -> Parser a
brackets = between (tokenP TokenLBracket) (tokenP TokenRBracket)

braces :: Parser a -> Parser a
braces = between (tokenP TokenLBrace) (tokenP TokenRBrace)

expr :: Parser LExpr
expr = undefined

bind :: Parser LBind
bind = located $ bindFun <|> bindName
  where
    bindFun = BindFun <$> (kw "fun" *> lowerIdent) <*> some lowerIdent <*> expr
    bindName = BindName <$> (kw "def" *> lowerIdent) <*> expr

mod :: Text -> Parser Module
mod name = Module name <$> many bind

interactive :: Parser (Either [LBind] LExpr)
interactive = undefined
