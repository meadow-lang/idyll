module Idyllic.Parser (parseInteractive, parseFile) where

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

int :: Parser Integer
int = token (\case (Located (TokenInt n) _) -> Just n; _ -> Nothing) Set.empty

bool :: Parser Bool
bool = token test Set.empty
  where
    test (Located (TokenUppercaseIdent name) _) =
      case name of
        "True" -> Just True
        "False" -> Just False
        _ -> Nothing
    test _ = Nothing

string :: Parser Text
string = token (\case (Located (TokenString str) _) -> Just str; _ -> Nothing) Set.empty

lit :: Parser Lit
lit =
  choice
    [ Int <$> int,
      Bool <$> bool,
      String <$> string
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
expr = app <|> atom
  where
    unit = tokenP TokenLParen *> tokenP TokenRParen $> ExprUnit
    litExpr = ExprLit <$> lit
    varExpr = ExprVar <$> lowerIdent
    lam = ExprLam <$> (tokenP TokenBackSlash *> some lowerIdent) <*> expr
    let' = ExprLet <$> (kw "let" *> lowerIdent) <*> (tokenP TokenEq *> expr) <*> (kw "in" *> expr)
    atom = located $ choice [unit, litExpr, let', varExpr, lam, parens (locItem <$> expr)]
    app = try $ located $ ExprApp <$> atom <*> some atom

bind :: Parser LBind
bind = located $ bindFun <|> bindName
  where
    bindFun = try $ BindFun <$> (kw "fun" *> lowerIdent) <*> some lowerIdent <*> (tokenP TokenEq *> expr)
    bindName = try $ BindName <$> (kw "def" *> lowerIdent) <*> (tokenP TokenEq *> expr)

mod' :: Text -> Parser Module
mod' name = Module name <$> many bind

interactive :: Parser (Either [LBind] LExpr)
interactive = Left <$> try (some bind) <|> Right <$> expr

parseFile :: Text -> [LToken] -> Either (ParseErrorBundle [LToken] Void) Prog
parseFile filename = parse (located $ mod' filename) (unpack filename)

parseInteractive :: [LToken] -> Either (ParseErrorBundle [LToken] Void) (Either [LBind] LExpr)
parseInteractive = parse (interactive <* eof) "<interactive>"
