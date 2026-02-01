module Idyllic.Syn.Parser (parseFile, parseInteractive) where

import Control.Monad (void)
import Control.Monad.Combinators.Expr
import Data.Functor (($>))
import Data.Text (Text, pack, unpack)
import Data.Void
import Error.Diagnose.Compat.Megaparsec (HasHints (..))
import Idyllic.Build.Pipeline.Effect
import Idyllic.Syn.AST
import Idyllic.Utils.Located
import Idyllic.Utils.Span
import Text.Megaparsec
import Text.Megaparsec.Char
import qualified Text.Megaparsec.Char.Lexer as L
import Text.Megaparsec.Debug (MonadParsecDbg (dbg))

type Parser = Parsec Void Text

instance HasHints Void msg where
  hints _ = mempty

parseFile :: Text -> Text -> Either (ParseErrorBundle Text Void) Prog
parseFile filename = parse (mod' filename) (unpack filename)

parseInteractive :: Text -> Either (ParseErrorBundle Text Void) (Either [LDecl] LExpr)
parseInteractive = parse interactive "<interactive>"

interactive :: Parser (Either [LDecl] LExpr)
interactive = sc *> choice [Left <$> many (L.nonIndented scn decl), Right <$> expr] <* eof

mod' :: Text -> Parser Prog
mod' filename = do
  ds <- located $ many (L.nonIndented scn decl) <* eof
  pure $ Located (Module filename (locItem ds)) (locSpan ds)

decl :: Parser LDecl
decl = dbg "decl" $ located $ DeclClassDecl <$> classDecl

classDecl :: Parser LClassDecl
classDecl = located $ ClassDeclSig <$> sig <|> ClassDeclBind <$> bind

sig :: Parser LSig
sig =
  dbg "sig" $
    located . try $
      Sig <$> (lowerCaseIdent `sepBy1` charL ',') <* charL ':' <*> tyVars <*> ann
  where
    tyVars = try (some lowerCaseIdent <* charL '.') <|> pure []

rhs :: Parser Rhs
rhs = choice [RhsGuard <$> some guard, RhsExpr <$> expr]

expr :: Parser LExpr
expr = dbg "expr" $ makeExprParser apply operatorTable
  where
    unit' = unit $> ExprUnit
    litExpr = ExprLit <$> lit
    varExpr = ExprVar <$> lowerCaseIdent

    simple :: Parser Expr
    simple = choice [litExpr, varExpr, unit', parens (locItem <$> expr)]

    lambda :: Parser Expr
    lambda = ExprLam <$> (char '\\' *> some pat) <* symbol "->" <*> expr

    let' :: Parser Expr
    let' = ExprLet <$> (kw "let" *> some bind) <* kw "in" <*> expr

    if' :: Parser Expr
    if' = ExprIf <$> (kw "if" *> expr) <* kw "then" <*> expr <* kw "else" <*> expr

    case' :: Parser Expr
    case' = undefined
    --   indentBlock scn $ do
    --     kwMatch
    --     scrut <- expr
    --     kwWith
    --     pure $ L.IndentSome Nothing (cont scrut) alt
    --   where
    --     alt :: Parser (LPat, LExpr)
    --     alt = (,) <$> pattern' <*> (symbol "->" *> expr)

    --     cont :: LExpr -> [(LPat, LExpr)] -> Parser Expr
    --     cont scr alts = pure $ Match scr alts

    list :: Parser Expr
    list = ExprList <$> brackets (expr `sepEndBy` char ',')

    tuple :: Parser Expr
    tuple = ExprTuple <$> try (parens ((:) <$> (expr <* char ',')) <*> expr `sepEndBy1` char ',')

    record :: Parser Expr
    record =
      ExprRecord
        <$> optional upperCaseIdent
        <*> braces (((,) <$> lowerCaseIdent <*> (char '=' *> expr)) `sepEndBy1` char ',')

    atom :: Parser LExpr
    atom =
      located $
        choice
          [ let',
            lambda,
            if',
            -- case',
            list,
            tuple <|> simple,
            record
          ]

    apply :: Parser LExpr
    apply = do
      f <- atom
      args <- many atom
      case args of
        [] -> pure f
        _ -> pure $ Located (ExprApp f args) (locSpan f <> locSpan (last args))

operatorTable :: [[Operator Parser LExpr]]
operatorTable =
  [ [prefix "-" (\s e -> Located (ExprNeg e) (s <> locSpan e))],
    [ binary "*" (\s l r -> Located (ExprBinary (Located BinOpMul s) l r) (locSpan l <> locSpan r)),
      binary "/" (\s l r -> Located (ExprBinary (Located BinOpDiv s) l r) (locSpan l <> locSpan r)),
      binary "%" (\s l r -> Located (ExprBinary (Located BinOpMod s) l r) (locSpan l <> locSpan r))
    ],
    [ binary "+" (\s l r -> Located (ExprBinary (Located BinOpAdd s) l r) (locSpan l <> locSpan r)),
      binary "-" (\s l r -> Located (ExprBinary (Located BinOpSub s) l r) (locSpan l <> locSpan r))
    ],
    [ binary "==" (\s l r -> Located (ExprBinary (Located BinOpEq s) l r) (locSpan l <> locSpan r)),
      binary "!=" (\s l r -> Located (ExprBinary (Located BinOpNeq s) l r) (locSpan l <> locSpan r)),
      binary "<" (\s l r -> Located (ExprBinary (Located BinOpLt s) l r) (locSpan l <> locSpan r)),
      binary "<=" (\s l r -> Located (ExprBinary (Located BinOpLte s) l r) (locSpan l <> locSpan r)),
      binary ">" (\s l r -> Located (ExprBinary (Located BinOpGt s) l r) (locSpan l <> locSpan r)),
      binary ">=" (\s l r -> Located (ExprBinary (Located BinOpGte s) l r) (locSpan l <> locSpan r))
    ],
    [ binary "&&" (\s l r -> Located (ExprBinary (Located BinOpAnd s) l r) (locSpan l <> locSpan r))
    ],
    [ binary "||" (\s l r -> Located (ExprBinary (Located BinOpOr s) l r) (locSpan l <> locSpan r))
    ]
  ]

binary :: Text -> (Span -> LExpr -> LExpr -> LExpr) -> Operator Parser LExpr
binary name f = InfixL (f . locSpan <$> located (symbol name))

prefix, postfix :: Text -> (Span -> LExpr -> LExpr) -> Operator Parser LExpr
prefix name f = Prefix (f . locSpan <$> located (string name))
postfix name f = Postfix (f . locSpan <$> located (string name))

bind :: Parser LBind
bind = dbg "bind" $ located $ funBind <|> patternBind
  where
    patternBind :: Parser Bind
    patternBind = BindPat <$> pat <* charL '=' <*> rhs <*> where'

    funBind :: Parser Bind
    funBind = try $ BindFun <$> lowerCaseIdent <*> some alt <*> where'

    alt :: Parser LAlt
    alt = located $ Alt <$> some pat <* charL '=' <*> rhs

    where' :: Parser [LClassDecl]
    where' = option [] (kw "where" *> some classDecl)

guard :: Parser LGuard
guard = located . try $ Guard <$> pat <* charL '|' <*> expr

ann :: Parser LAnn
ann = arrowType <|> baseType
  where
    arrowType :: Parser LAnn
    arrowType = located $ try (AnnFun <$> baseType <* symbol "->") <*> ann

    baseType :: Parser LAnn
    baseType =
      located $
        choice
          [ varType,
            identType,
            listType,
            unit
              $> AnnUnit
                <|> tupleType
                <|> parens (locItem <$> ann)
          ]

    varType = AnnVar <$> lowerCaseIdent
    identType = AnnIdent <$> upperCaseIdent
    listType = AnnList <$> brackets ann
    tupleType =
      AnnTuple
        <$> try (parens ((:) <$> (ann <* char ',')) <*> ann `sepEndBy1` char ',')

pat :: Parser LPat
pat = located $ choice [wildcard, litP, identP, consP, listP, unitP]
  where
    wildcard = char '_' $> PatWildcard
    litP = PatLit <$> lit
    identP = PatIdent <$> lowerCaseIdent
    consP = PatCons <$> (upperCaseIdent <* char '@') <*> some pat
    listP = PatList <$> brackets (pat `sepEndBy` char ',')
    unitP = unit $> PatUnit

parens :: Parser a -> Parser a
parens = between (lexeme $ char '(') (lexeme $ char ')')

brackets :: Parser a -> Parser a
brackets = between (lexeme $ char '[') (lexeme $ char ']')

braces :: Parser a -> Parser a
braces = between (lexeme $ char '{') (lexeme $ char '}')

lowerCaseIdent :: Parser Ident
lowerCaseIdent = dbg "lowerCaseIdent" $ try $ do
  name <- located $ pack <$> ((:) <$> identStartChar <*> many identChar)
  let !sv = locItem name
  if sv `elem` keywords
    then fail $ "keyword " ++ unpack sv ++ " cannot be an identifier"
    else pure name
  where
    identStartChar = lowerChar <|> char '_'
    identChar = alphaNumChar <|> char '_' <|> char '\''

    keywords :: [Text]
    keywords =
      [ "mod",
        "use",
        "as",
        "let",
        "in",
        "where",
        "if",
        "then",
        "else",
        "case",
        "of",
        "record",
        "data",
        "type",
        "class",
        "instance",
        "do"
      ]

upperCaseIdent :: Parser Ident
upperCaseIdent = located (pack <$> ((:) <$> upperChar <*> many alphaNumChar))

kw :: Text -> Parser ()
kw name = symbol name $> () <?> unpack name

lexeme :: Parser a -> Parser a
lexeme = L.lexeme sc

symbol :: Text -> Parser Text
symbol = L.symbol sc

charL :: Char -> Parser Char
charL c = lexeme (char c)

octal :: Parser Integer
octal = try (char '0' *> char' 'o') *> L.octal

hexadecimal :: Parser Integer
hexadecimal = try (char '0' >> char' 'x') >> L.hexadecimal

int :: Parser Integer
int = choice [octal, hexadecimal, L.decimal]

bool :: Parser Bool
bool = choice [True <$ string "true", False <$ string "false"]

str :: Parser Text
str = char '\"' *> (pack <$> manyTill L.charLiteral (char '\"'))

lit :: Parser Lit
lit =
  choice
    [ Int <$> int,
      Bool <$> bool,
      String <$> str
    ]

unit :: Parser ()
unit = symbol "()" $> ()

scn :: Parser ()
scn = L.space space1 lineComment empty

sc :: Parser ()
sc = L.space (void $ some (char ' ' <|> char '\t')) lineComment empty

lineComment :: Parser ()
lineComment = L.skipLineComment "--"

located :: Parser a -> Parser (Located a)
located p = do
  start <- getOffset
  res <- p
  end <- getOffset
  sc
  pure $ Located res (fromPair (start, end))

-- located :: Parser a -> Parser (Located a)
-- located p = do
--   input <- getInput
--   --   startOff <- getOffset
--   let start = locSpan $ head input
--   res <- p
--   --   endOff <- getOffset
--   let end = locSpan $ head input
--   pure $ Located res (start <> end)

-- letExpr :: Parser Expr
-- letExpr = do
--   _ <- symbol "let"
--   -- Check for '{' immediately
--   isExplicit <- optional (symbol "{")
--   case isExplicit of
--     Just _ -> do
--       -- Standard non-layout parsing
--       binds <- binding `sepBy` symbol ";"
--       _ <- symbol "}"
--       _ <- symbol "in"
--       body <- expr
--       return $ Let binds body
--     Nothing -> do
--       -- Layout-sensitive parsing via indentBlock
--       binds <- L.indentBlock scn $
--         return (L.IndentMany Nothing return binding)
--       _ <- symbol "in"
--       body <- expr
--       return $ Let binds body