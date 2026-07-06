{-# OPTIONS_GHC -Wno-orphans #-}

module Idyllic.Syntax.Parser (parseInteractive) where

import Data.List (unsnoc)
import Data.Set qualified as Set
import Data.Text (Text)
import Data.Void
import Idyllic.Syntax.AST
import Idyllic.Syntax.Token
import Idyllic.Utils
import Text.Megaparsec hiding (Token)

type Parser = Parsec Void [LToken]

-- parseFile :: Text -> [LToken] -> Either (ParseErrorBundle [LToken] Void) Prog
-- parseFile filename = parse (cfg <|> layout) (unpack filename)
--   where
--     cfg = try $ located $ braces (mod' filename)
--     layout = located $ vbraces (mod' filename)

parseInteractive :: [LToken] -> Either (ParseErrorBundle [LToken] Void) LTerm
parseInteractive = parse (interactive <* eof) "<interactive>"

interactive :: Parser LTerm
interactive = cfgTerm <|> layoutTerm
  where
    cfgTerm = try $ braces term
    layoutTerm = vbraces term

term :: Parser LTerm
term = app <|> atom
  where
    litTerm = TermLit <$> lit
    varTerm = TermVar <$> ident
    lam = TermLam <$> (tokenP TokenBackSlash *> some lowerIdent) <*> (tokenP TokenRArrow *> term)
    let' = TermLet <$> (tokenP TokenLet *> binds) <*> (tokenP TokenIn *> term)
    atom = located $ choice [try litTerm, let', varTerm, lam, ann, parens (unLoc <$> term)]
    app = try $ located $ TermApp <$> atom <*> some atom
    ann = try $ TermAnn <$> atom <*> (tokenP TokenColon *> term)

binds :: Parser [LBind]
binds = cfgBind <|> layoutBind
  where
    cfgBind = try $ braces (bind `sepEndBy1` tokenP TokenSemi)
    layoutBind = try $ vbraces (bind `sepEndBy1` tokenP TokenVSemi)

bind :: Parser LBind
bind = located $ bindFun <|> bindName
  where
    bindFun = try $ BindFun <$> lowerIdent <*> some lowerIdent <*> (tokenP TokenEq *> term)
    bindName = try $ BindName <$> lowerIdent <*> (tokenP TokenEq *> term)

parens :: Parser a -> Parser a
parens = between (tokenP TokenLParen) (tokenP TokenRParen)

brackets :: Parser a -> Parser a
brackets = between (tokenP TokenLBracket) (tokenP TokenRBracket)

braces :: Parser a -> Parser a
braces = between (tokenP TokenLBrace) (tokenP TokenRBrace)

vbraces :: Parser a -> Parser a
vbraces = between (tokenP TokenVLBrace) (tokenP TokenVRBrace)

ident :: Parser Ident
ident = choice [lowerIdent, upperIdent]

lowerIdent :: Parser Ident
lowerIdent = token (\case (Located (TokenLowercaseIdent n) s) -> Just (Located n s); _ -> Nothing) Set.empty

upperIdent :: Parser Ident
upperIdent = token (\case (Located (TokenUppercaseIdent n) s) -> Just (Located n s); _ -> Nothing) Set.empty

lit :: Parser Lit
lit =
  choice
    [ LitUnit <$ tokenP TokenLParen <* tokenP TokenRParen,
      LitInt <$> int,
      LitBool <$> bool,
      LitString <$> string
    ]

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

tokenP :: Token -> Parser Token
tokenP t = token (\(Located lt _) -> if t == lt then Just t else Nothing) Set.empty

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
