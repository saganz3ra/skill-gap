import { useCallback, useEffect, useState } from "react";

interface Resource<T> {
  data: T | undefined;
  error: unknown;
  loading: boolean;
  reload: () => Promise<void>;
  /** Para atualização otimista: muda o dado local antes do backend responder. */
  setData: React.Dispatch<React.SetStateAction<T | undefined>>;
}

/** Carrega um dado assíncrono uma vez ao montar, com recarga manual. */
export function useResource<T>(load: () => Promise<T>): Resource<T> {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<unknown>(null);
  const [loading, setLoading] = useState(true);

  const reload = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setData(await load());
    } catch (e) {
      setError(e);
    } finally {
      setLoading(false);
    }
  }, [load]);

  useEffect(() => {
    void reload();
  }, [reload]);

  return { data, error, loading, reload, setData };
}
