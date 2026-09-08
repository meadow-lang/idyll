{-# LANGUAGE TypeFamilies #-}

-- | A megaparsec 'Stream' over the lexer's @['LToken']@, paired with the
--   original source text.
--
--   megaparsec ships a generic @instance Ord a => Stream [a]@, but it tracks
--   position purely by token /index/ (every error would land on \"line 1,
--   column N\").  This wrapper instead threads each token's real 'Span'
--   through to megaparsec's 'SourcePos', and, because it keeps the source
--   text around, 'errorBundlePretty' can show the offending source line with
--   a caret of the right width.
module Idyllic.Syntax.Stream
  ( TokenStream (..),
    mkTokenStream,
  )
where

import Data.ByteString qualified as BS
import Data.List.NonEmpty qualified as NE
import Data.Proxy (Proxy (..))
import Data.Text (Text)
import Data.Text qualified as T
import Data.Text.Encoding qualified as TE
import Idyllic.Syntax.Token
import Idyllic.Utils (Located (..), Span (..))
import Text.Megaparsec

-- | The parser input: remaining tokens plus the whole source text (constant;
--   only used for rendering error context).
data TokenStream = TokenStream
  { tsSource :: Text,
    tsTokens :: [LToken]
  }
  deriving (Show)

mkTokenStream :: Text -> [LToken] -> TokenStream
mkTokenStream = TokenStream

instance Stream TokenStream where
  type Token TokenStream = LToken
  type Tokens TokenStream = [LToken]

  tokenToChunk Proxy t = [t]
  tokensToChunk Proxy = id
  chunkToTokens Proxy = id
  chunkLength Proxy = length
  chunkEmpty Proxy = null

  take1_ (TokenStream _ []) = Nothing
  take1_ (TokenStream src (t : ts)) = Just (t, TokenStream src ts)

  takeN_ n s@(TokenStream src ts)
    | n <= 0 = Just ([], s)
    | null ts = Nothing
    | otherwise = let (taken, ts') = splitAt n ts in Just (taken, TokenStream src ts')

  takeWhile_ p (TokenStream src ts) =
    let (taken, ts') = span p ts in (taken, TokenStream src ts')

instance VisualStream TokenStream where
  showTokens Proxy = unwords . map (showToken . unLoc) . NE.toList
  tokensLength Proxy = sum . fmap (spanWidth . locSpan)
    where
      spanWidth (Span a b) = max 1 (b - a)

instance TraversableStream TokenStream where
  reachOffset o pst =
    let (mLine, textPst) = reachOffset (targetCharOffset pst o) (freshTextPosState pst)
     in (mLine, rebuild pst o textPst)

  reachOffsetNoLine o pst =
    rebuild pst o (reachOffsetNoLine (targetCharOffset pst o) (freshTextPosState pst))

-- | Character offset into the source text of the token we are about to
--   reach (its 'Span' start), or the end of the last consumed token, or the
--   very start of input.
--
--   'Span's are byte offsets into the UTF-8 encoding; convert to a codepoint
--   offset so it lines up with 'Data.Text' indexing.
targetCharOffset :: PosState TokenStream -> Int -> Int
targetCharOffset pst o =
  T.length (TE.decodeUtf8Lenient (BS.take byteOffset (TE.encodeUtf8 src)))
  where
    src = tsSource (pstateInput pst)
    toks = tsTokens (pstateInput pst)
    step = o - pstateOffset pst
    byteOffset = case drop step toks of
      (Located _ (Span s _) : _) -> s
      [] -> case reverse (take step toks) of
        (Located _ (Span _ e) : _) -> e
        [] -> 0

-- | A 'PosState' over the raw source 'Text', anchored at the start, so that
--   megaparsec's well-tested 'Text' line extraction can be reused.
freshTextPosState :: PosState TokenStream -> PosState Text
freshTextPosState pst =
  PosState
    { pstateInput = tsSource (pstateInput pst),
      pstateOffset = 0,
      pstateSourcePos = initialPos (sourceName (pstateSourcePos pst)),
      pstateTabWidth = pstateTabWidth pst,
      pstateLinePrefix = ""
    }

-- | Splice the result of delegating to the 'Text' stream back into a
--   'TokenStream' 'PosState'.
rebuild :: PosState TokenStream -> Int -> PosState Text -> PosState TokenStream
rebuild pst o textPst =
  PosState
    { pstateInput = TokenStream (tsSource (pstateInput pst)) (drop step toks),
      pstateOffset = max (pstateOffset pst) o,
      pstateSourcePos = pstateSourcePos textPst,
      pstateTabWidth = pstateTabWidth pst,
      pstateLinePrefix = pstateLinePrefix textPst
    }
  where
    toks = tsTokens (pstateInput pst)
    step = o - pstateOffset pst
