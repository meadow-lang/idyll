module Idyllic.Comb where

import Control.Applicative (Alternative (..))
import Control.Monad.Identity (Identity)
import Data.Text (Text)
import qualified Data.Text as Text

data Parsecesult e a = Parsecesult
  { resultValue :: Maybe a, -- The partial or successful output
    remaining :: Text, -- The rest of the input
    errors :: [e] -- Accumulated errors
  }
  deriving (Show)

newtype ParsecT e m a = ParsecT
  {runParsecT :: Text -> m (Parsecesult e a)}

type Parsec a e = ParsecT e Identity a

instance (Monad m) => Functor (ParsecT e m) where
  fmap f (ParsecT p) = ParsecT $ \input -> do
    res <- p input
    pure res {resultValue = fmap f (resultValue res)}

instance (Monad m) => Applicative (ParsecT e m) where
  pure x = ParsecT $ \input -> pure $ Parsecesult (Just x) input []

  (ParsecT pf) <*> (ParsecT pa) = ParsecT $ \input -> do
    resF <- pf input
    case resultValue resF of
      Nothing -> pure resF {resultValue = Nothing} -- Propagate failure
      Just f -> do
        resA <- pa (remaining resF)
        pure resA {resultValue = fmap f (resultValue resA), errors = errors resF ++ errors resA}

instance (Monad m) => Alternative (ParsecT e m) where
  empty = ParsecT $ \input -> pure $ Parsecesult Nothing input []
  (ParsecT p1) <|> (ParsecT p2) = ParsecT $ \input -> do
    res1 <- p1 input
    case resultValue res1 of
      Just _ -> pure res1
      Nothing -> do
        res2 <- p2 input
        pure res2 {errors = errors res1 ++ errors res2}

instance (Monad m) => Monad (ParsecT e m) where
  return = pure

  (ParsecT pa) >>= f =
    ParsecT $ \input -> do
      resA <- pa input
      case resultValue resA of
        Nothing -> pure resA {resultValue = Nothing} -- Propagate failure
        Just a -> runParsecT (f a) (remaining resA)

(<?>) :: ParsecT e m a -> Text -> ParsecT e m a
(ParsecT p) <?> errMsg = ParsecT $ \input -> do
  res <- p input
  case resultValue res of
    Just _ -> pure res
    Nothing -> pure res {errors = errors res ++ [errorMessage errMsg]}

anyChar :: Parsec Char e
anyChar = ParsecT $ \input ->
  case Text.uncons input of
    Nothing -> pure $ Parsecesult Nothing input []
    Just (c, rest) -> pure $ Parsecesult (Just c) rest []

satisfy :: (Char -> Bool) -> Parsec Char e
satisfy predicate = do
  c <- anyChar
  if predicate c
    then return c
    else ParsecT $ \input -> pure $ Parsecesult Nothing input [errorMessage "Unexpected character"]
