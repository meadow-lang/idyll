{-# OPTIONS_GHC -Wno-orphans #-}

module Idyllic.Parser (parseInteractive, parseFile) where

import Control.Monad (void)
import Control.Monad.Combinators.Expr
import Data.Functor (($>))
import Data.List (unsnoc)
import Data.List.NonEmpty qualified as NE
import Data.List.NonEmpty qualified as NonEmpty
import Data.Maybe (fromMaybe, isJust)
import Data.Proxy
import Data.Set qualified as Set
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

vbraces :: Parser a -> Parser a
vbraces = between (tokenP TokenVLBrace) (tokenP TokenVRBrace)

expr :: Parser LExpr
expr = app <|> atom
  where
    unit = tokenP TokenLParen *> tokenP TokenRParen $> ExprUnit
    litExpr = ExprLit <$> lit
    varExpr = ExprVar <$> lowerIdent
    lam = ExprLam <$> (tokenP TokenBackSlash *> some lowerIdent) <*> (tokenP TokenRArrow *> expr)
    let' = ExprLet <$> (tokenP TokenLet *> binds) <*> (tokenP TokenIn *> expr)
    atom = located $ choice [try unit, litExpr, let', varExpr, lam, parens (unLoc <$> expr)]
    app = try $ located $ ExprApp <$> atom <*> some atom

binds :: Parser [LBind]
binds = cfgBind <|> layoutBind
  where
    cfgBind = try $ braces (bind `sepEndBy1` tokenP TokenSemi)
    layoutBind = try $ vbraces (bind `sepEndBy1` tokenP TokenVSemi)

bind :: Parser LBind
bind = located $ bindFun <|> bindName
  where
    bindFun = try $ BindFun <$> lowerIdent <*> some lowerIdent <*> (tokenP TokenEq *> expr)
    bindName = try $ BindName <$> lowerIdent <*> (tokenP TokenEq *> expr)

mod' :: Text -> Parser Module
mod' name = Module name <$> many bind

interactive :: Parser (Either [LBind] LExpr)
interactive = Left <$> binds <|> Right <$> (cfgExpr <|> layoutExpr)
  where
    cfgExpr = try $ braces expr
    layoutExpr = vbraces expr

parseFile :: Text -> [LToken] -> Either (ParseErrorBundle [LToken] Void) Prog
parseFile filename = parse (cfg <|> layout) (unpack filename)
  where
    cfg = try $ located $ braces (mod' filename)
    layout = located $ vbraces (mod' filename)

parseInteractive :: [LToken] -> Either (ParseErrorBundle [LToken] Void) (Either [LBind] LExpr)
parseInteractive = parse (interactive <* eof) "<interactive>"
