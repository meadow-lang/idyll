{-# OPTIONS_GHC -Wno-orphans #-}

module Idyllic.Parser (parseInteractive, parseFile) where

-- import Control.Monad (void)
-- import Control.Monad.Combinators.Expr
import Data.Functor (($>))
import Data.List (unsnoc)
-- import Data.List.NonEmpty qualified as NE
-- import Data.List.NonEmpty qualified as NonEmpty
-- import Data.Maybe (fromMaybe, isJust)
-- import Data.Proxy
import Data.Set qualified as Set
import Data.Text (Text, unpack)
import Data.Void
-- import Error.Diagnose.Compat.Megaparsec (HasHints (..))
import Idyllic.AST
-- import Idyllic.Effect
import Idyllic.Token
import Idyllic.Utils
import Text.Megaparsec hiding (Token)

type Parser = Parsec Void [LToken]

parseFile :: Text -> [LToken] -> Either (ParseErrorBundle [LToken] Void) Prog
parseFile filename = parse (cfg <|> layout) (unpack filename)
  where
    cfg = try $ located $ braces (mod' filename)
    layout = located $ vbraces (mod' filename)

parseInteractive :: [LToken] -> Either (ParseErrorBundle [LToken] Void) (Either [LBind] LTerm)
parseInteractive = parse (interactive <* eof) "<interactive>"

interactive :: Parser (Either [LBind] LTerm)
interactive = Left <$> binds <|> Right <$> (cfgTerm <|> layoutTerm)
  where
    cfgTerm = try $ braces term
    layoutTerm = vbraces term

mod' :: Text -> Parser Module
mod' name = Module name <$> many decl

decl :: Parser LDecl
decl = located $ choice [try declUse]
  where
    declUse = DeclUse <$> (tokenP TokenUse *> some upperIdent)

term :: Parser LTerm
term = app <|> atom
  where
    unit = tokenP TokenLParen *> tokenP TokenRParen $> TermUnit
    litTerm = TermLit <$> lit
    varTerm = TermVar <$> ident
    lam = TermLam <$> (tokenP TokenBackSlash *> some pat) <*> (tokenP TokenRArrow *> term)
    let' = TermLet <$> (tokenP TokenLet *> binds) <*> (tokenP TokenIn *> term)
    atom = located $ choice [try unit, litTerm, let', varTerm, lam, parens (unLoc <$> term)]
    app = try $ located $ TermApp <$> atom <*> some atom

binds :: Parser [LBind]
binds = cfgBind <|> layoutBind
  where
    cfgBind = try $ braces (bind `sepEndBy1` tokenP TokenSemi)
    layoutBind = try $ vbraces (bind `sepEndBy1` tokenP TokenVSemi)

bind :: Parser LBind
bind = located $ bindFun <|> bindName
  where
    bindFun = try $ BindFun <$> lowerIdent <*> some pat <*> (tokenP TokenEq *> rhs)
    bindName = try $ BindPat <$> pat <*> (tokenP TokenEq *> rhs)

rhs :: Parser Rhs
rhs = RhsTerm <$> term

pat :: Parser LPat
pat = located $ choice [try patWildcard, patLit, patIdent, patCons, patAs, patList, patTuple, patUnit]
  where
    patWildcard = tokenP TokenUnderscore $> PatWildcard
    patLit = PatLit <$> lit
    patIdent = PatIdent <$> lowerIdent
    patCons = PatCons <$> upperIdent <*> many pat
    patAs = PatAs <$> lowerIdent <*> (tokenP TokenAt *> pat)
    patList = PatList <$> brackets (pat `sepBy` tokenP TokenComma)
    patTuple = PatTuple <$> parens (pat `sepBy` tokenP TokenComma)
    patUnit = tokenP TokenLParen *> tokenP TokenRParen $> PatUnit

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
lowerIdent = token (\case (Located (TokenLowercaseIdent n) s) -> Just (Located (Name NameLower n) s); _ -> Nothing) Set.empty

upperIdent :: Parser Ident
upperIdent = token (\case (Located (TokenUppercaseIdent n) s) -> Just (Located (Name NameUpper n) s); _ -> Nothing) Set.empty

lit :: Parser Lit
lit =
  choice
    [ LitInt <$> int,
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
