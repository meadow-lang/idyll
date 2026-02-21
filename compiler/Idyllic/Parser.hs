{-# OPTIONS_GHC -Wno-orphans #-}

module Idyllic.Parser (parseInteractive, parseFile) where

import Control.Monad (void)
import Control.Monad.Combinators.Expr
import Data.Functor (($>))
import Data.List (unsnoc)
import qualified Data.List.NonEmpty as NE
import qualified Data.List.NonEmpty as NonEmpty
import Data.Maybe (fromMaybe, isJust)
import Data.Proxy
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

-- instance VisualStream [LToken] where
--   -- showTokens Proxy = show
--   showTokens _ = unwords . map show . NE.toList

-- instance TraversableStream [LToken] where
--   reachOffset o PosState {..} =
--     ( Just (prefix ++ restOfLine),
--       PosState
--         { pstateInput = post,
--           pstateOffset = max pstateOffset o,
--           pstateSourcePos = newSourcePos,
--           pstateTabWidth = pstateTabWidth,
--           pstateLinePrefix = prefix
--         }
--     )
--     where
--       prefix =
--         if sameLine
--           then pstateLinePrefix ++ preLine
--           else preLine
--       preLine = reverse . takeWhile (/= '\n') . reverse $ preStr
--       (preStr, postStr) = splitAt tokensConsumed (unpack $ streamSrc pstateInput)
--       newSourcePos =
--         case post of
--           [] -> case pstateInput of
--             [] -> pstateSourcePos
--             ts -> let (l,c) = offsetToLineCol $ spanEnd (last ts) in SourcePos (sourceName pstateSourcePos) l c
--           (x : _) -> wpStart x
--       sameLine = sourceLine newSourcePos == sourceLine pstateSourcePos
--       (pre, post) = splitAt (o - pstateOffset) pstateInput
--       tokensConsumed =
--         case NE.nonEmpty pre of
--           Nothing -> 0
--           Just nePre -> tokensLength pxy nePre
--       restOfLine = takeWhile (/= '\n') postStr
--       pxy :: Proxy [LToken]
--       pxy = Proxy

-- instance HasHints Void msg where
--   hints _ = mempty

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
    let' = ExprLet <$> (tokenP TokenLet *> (cfgLet <|> letBinds)) <*> (tokenP TokenIn *> expr)
      where
        cfgLet = try $ braces (bind `sepEndBy` tokenP TokenSemi)
        letBinds = vbraces (bind `sepEndBy` tokenP TokenVSemi)
    atom = located $ choice [try unit, litExpr, let', varExpr, lam, parens (locItem <$> expr)]
    app = try $ located $ ExprApp <$> atom <*> some atom

bind :: Parser LBind
bind = located $ bindFun <|> bindName
  where
    bindFun = try $ BindFun <$> lowerIdent <*> some lowerIdent <*> (tokenP TokenEq *> expr)
    bindName = try $ BindName <$> lowerIdent <*> (tokenP TokenEq *> expr)

mod' :: Text -> Parser Module
mod' name = Module name <$> many bind

interactive :: Parser (Either [LBind] LExpr)
interactive = Left <$> (cfgBind <|> layoutBind) <|> Right <$> (cfgExpr <|> layoutExpr)
  where
    cfgBind = try $ braces (bind `sepEndBy1` tokenP TokenSemi)
    layoutBind = try $ vbraces (bind `sepEndBy1` tokenP TokenVSemi)
    cfgExpr = try $ braces expr
    layoutExpr = vbraces expr

parseFile :: Text -> [LToken] -> Either (ParseErrorBundle [LToken] Void) Prog
parseFile filename = parse (cfg <|> layout) (unpack filename)
  where
    cfg = try $ located $ braces (mod' filename)
    layout = located $ vbraces (mod' filename)

parseInteractive :: [LToken] -> Either (ParseErrorBundle [LToken] Void) (Either [LBind] LExpr)
parseInteractive = parse (interactive <* eof) "<interactive>"
