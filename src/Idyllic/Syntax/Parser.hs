-- | Surface parser for the CoC-style core.
--
--   The grammar, loosest to tightest:
--
--   > term    ::= lambda | let | piOrArrow
--   > lambda  ::= '\' binder+ '->' term
--   > let     ::= 'let' binds 'in' term
--   > piOrArrow
--   >         ::= domain+ '->' term            -- dependent: (x : a) (y : b) -> c
--   >           | annExpr ('->' term)?         -- non-dependent arrow, right assoc
--   > annExpr ::= appExpr (':' term)?
--   > appExpr ::= atom atom*                   -- application, left assoc
--   > atom    ::= 'Type' | '_' | literal | ident | '(' term ')'
--   > domain  ::= '(' lowerIdent+ ':' term ')'
--   > binder  ::= lowerIdent | '(' lowerIdent+ (':' term)? ')'
--
--   Binders (lambda parameters, Pi domains, @let@ names) are lower-case,
--   Haskell-style; an upper-case name is only a reference (@atom@).
module Idyllic.Syntax.Parser
  ( Parser,
    parseInteractive,
    parseTerm,
    prettyParseError,
  )
where

import Data.Set qualified as Set
import Data.Text (Text)
import Data.Void (Void)
import Idyllic.Syntax.AST
import Idyllic.Syntax.Stream
import Idyllic.Syntax.Token
import Idyllic.Utils
import Text.Megaparsec hiding (Token)
import Text.Megaparsec qualified as MP

type Parser = Parsec Void TokenStream

-- ---------------------------------------------------------------------------
-- Entry points
-- ---------------------------------------------------------------------------

-- | Parse a term from a post-layout token stream (see
--   'Idyllic.Syntax.Layout.layout') plus the original source (used only to
--   render error context).
parseInteractive :: Text -> [LToken] -> Either (ParseErrorBundle TokenStream Void) LTerm
parseInteractive = parseTerm "<interactive>"

parseTerm :: FilePath -> Text -> [LToken] -> Either (ParseErrorBundle TokenStream Void) LTerm
parseTerm name src toks =
  parse (topTerm <* eof) name (mkTokenStream src (stripNewlines toks))

prettyParseError :: ParseErrorBundle TokenStream Void -> String
prettyParseError = errorBundlePretty

-- | Post-layout, bare newline tokens carry no meaning (the layout algorithm
--   has already turned the significant ones into virtual @;@/@{@/@}@), but a
--   few can survive; drop them so the grammar need not mention them.
stripNewlines :: [LToken] -> [LToken]
stripNewlines = filter ((/= TokenNewline) . unLoc)

-- | The layout pass always wraps the input in a virtual block, so the normal
--   path is @vbraces term@; @braces@ handles an explicit @{ ... }@.
topTerm :: Parser LTerm
topTerm = try (braces term) <|> vbraces term

-- ---------------------------------------------------------------------------
-- Grammar
-- ---------------------------------------------------------------------------

term :: Parser LTerm
term = choice [lambda, letExpr, piOrArrow]

piOrArrow :: Parser LTerm
piOrArrow = do
  mDomains <- optional (try (some domain <* lookAhead arrow))
  case mDomains of
    Just groups -> do
      _ <- arrow
      body <- term
      pure (foldr piFromParam body (concat groups))
    Nothing -> do
      lhs <- annExpr
      mArr <- optional arrow
      case mArr of
        Nothing -> pure lhs
        Just _ -> do
          rhs <- term
          pure (Located (TermPi Nothing lhs rhs) (locSpan lhs <> locSpan rhs))
  where
    piFromParam (Located (Param n ty) nsp) body =
      let dom = maybe body id ty -- 'domain' always supplies a type
       in Located (TermPi (Just n) dom body) (nsp <> locSpan body)

annExpr :: Parser LTerm
annExpr = do
  e <- appExpr
  mTy <- optional (colon *> term)
  case mTy of
    Nothing -> pure e
    Just ty -> pure (Located (TermAnn e ty) (locSpan e <> locSpan ty))

appExpr :: Parser LTerm
appExpr = do
  hd <- atom
  args <- many atom
  pure (foldl mkApp hd args)
  where
    mkApp f a = Located (TermApp f a) (locSpan f <> locSpan a)

atom :: Parser LTerm
atom =
  choice
    [ located (TermType <$ sym TokenType),
      located (TermHole <$ sym TokenUnderscore),
      located (TermLit <$> literal),
      located (TermVar <$> anyIdent),
      parens term
    ]
    <?> "atom"

literal :: Parser Lit
literal =
  choice
    [ LitInt <$> payload (\case TokenInt n -> Just n; _ -> Nothing),
      LitString <$> payload (\case TokenString s -> Just s; _ -> Nothing),
      try (LitUnit <$ (lparen *> rparen)),
      payload
        ( \case
            TokenUppercaseIdent "True" -> Just (LitBool True)
            TokenUppercaseIdent "False" -> Just (LitBool False)
            _ -> Nothing
        )
    ]

lambda :: Parser LTerm
lambda = do
  bslash <- sym TokenBackSlash <?> "'\\'"
  params <- concat <$> some binder
  _ <- arrow
  body <- term
  let sp = bslash <> locSpan body
  pure (foldr (\p acc -> Located (TermLam p acc) sp) body params)

letExpr :: Parser LTerm
letExpr = do
  letSp <- sym TokenLet
  bs <- letBinds
  _ <- sym TokenIn <?> "'in'"
  body <- term
  pure (Located (TermLet bs body) (letSp <> locSpan body))

letBinds :: Parser [LBind]
letBinds =
  choice
    [ try (braces (semiSep bind)),
      try (vbraces (semiSep bind)),
      semiSep bind
    ]
  where
    -- Accept either the layout-inserted virtual @;@ or an explicit @;@, so a
    -- one-line @let a = ..; b = .. in ..@ and a laid-out block both work.
    semiSep p = p `sepEndBy1` (sym TokenVSemi <|> sym TokenSemi)

bind :: Parser LBind
bind = located (try bindFun <|> bindName)
  where
    bindFun = do
      f <- lowerIdent
      ps <- concat <$> some binder
      ty <- optional (colon *> term)
      _ <- sym TokenEq <?> "'='"
      BindFun f ps ty <$> term
    bindName = do
      x <- lowerIdent
      ty <- optional (colon *> term)
      _ <- sym TokenEq <?> "'='"
      BindName x ty <$> term

-- | A dependent-function domain: @(x y : a)@.  Always annotated; binders are
--   lower-case.
--
--   Each parameter is given the span of the whole parenthesised group, so a
--   Pi built from it reads as @(x : a) -> b@ rather than starting mid-token.
domain :: Parser [LParam]
domain = do
  Located ps sp <- located . parens $ do
    names <- some lowerIdent
    _ <- colon
    ty <- term
    pure [(n, ty) | n <- names]
  pure [Located (Param n (Just ty)) sp | (n, ty) <- ps]

-- | A lambda / definition binder: a bare lower-case name, or a parenthesised
--   group with an optional annotation.
binder :: Parser [LParam]
binder =
  choice
    [ pure <$> located ((`Param` Nothing) <$> lowerIdent),
      do
        Located ps sp <- located . parens $ do
          names <- some lowerIdent
          ty <- optional (colon *> term)
          pure [(n, ty) | n <- names]
        pure [Located (Param n ty) sp | (n, ty) <- ps]
    ]

-- ---------------------------------------------------------------------------
-- Token helpers
-- ---------------------------------------------------------------------------

-- | Consume the token whose payload @f@ accepts, keeping its span.
satisfyTok :: (Token -> Maybe a) -> Parser (Located a)
satisfyTok f = MP.token (\(Located t s) -> (`Located` s) <$> f t) Set.empty

-- | Like 'satisfyTok' but discards the span.
payload :: (Token -> Maybe a) -> Parser a
payload f = unLoc <$> satisfyTok f

-- | Consume exactly this token, returning its span.
sym :: Token -> Parser Span
sym want = MP.token (\(Located t s) -> if t == want then Just s else Nothing) Set.empty

lowerIdent :: Parser Ident
lowerIdent = satisfyTok (\case TokenLowercaseIdent n -> Just n; _ -> Nothing) <?> "identifier"

upperIdent :: Parser Ident
upperIdent = satisfyTok (\case TokenUppercaseIdent n -> Just n; _ -> Nothing) <?> "constructor"

anyIdent :: Parser Ident
anyIdent = lowerIdent <|> upperIdent

lparen, rparen, arrow, colon :: Parser Span
lparen = sym TokenLParen <?> "'('"
rparen = sym TokenRParen <?> "')'"
arrow = sym TokenRArrow <?> "'->'"
colon = sym TokenColon <?> "':'"

parens :: Parser a -> Parser a
parens = between lparen rparen

braces :: Parser a -> Parser a
braces = between (sym TokenLBrace <?> "'{'") (sym TokenRBrace <?> "'}'")

vbraces :: Parser a -> Parser a
vbraces = between (sym TokenVLBrace) (sym TokenVRBrace)

-- | Wrap a parser's result with the span covering exactly the tokens it
--   consumed.
located :: Parser a -> Parser (Located a)
located p = do
  start <- getOffset
  toks <- tsTokens <$> getInput
  x <- p
  end <- getOffset
  let consumed = take (end - start) toks
      sp = case map locSpan consumed of
        [] -> case toks of
          (Located _ s : _) -> Span (spanStart s) (spanStart s)
          [] -> Span 0 0
        (s0 : rest) -> foldl (<>) s0 rest
  pure (Located x sp)
