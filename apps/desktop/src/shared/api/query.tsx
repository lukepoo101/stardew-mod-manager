import React, { createContext, useContext, useEffect, useRef, useState, useCallback } from "react";

type QueryKey = readonly unknown[];

interface QueryCacheEntry<T = unknown> {
  data: T | undefined;
  error: Error | null;
  status: "idle" | "loading" | "success" | "error";
  updatedAt: number;
}

function serializeKey(key: QueryKey): string {
  return JSON.stringify(key);
}

export class QueryClient {
  private cache = new Map<string, QueryCacheEntry<any>>();
  private listeners = new Map<string, Set<() => void>>();
  private globalListeners = new Set<() => void>();

  getQueryData<T>(key: QueryKey): T | undefined {
    return this.cache.get(serializeKey(key))?.data as T | undefined;
  }

  setQueryData<T>(key: QueryKey, data: T | ((old: T | undefined) => T)): void {
    const serialized = serializeKey(key);
    const existing = this.cache.get(serialized);
    const newData = typeof data === "function" ? (data as any)(existing?.data) : data;
    this.cache.set(serialized, {
      data: newData,
      error: null,
      status: "success",
      updatedAt: Date.now(),
    });
    this.notify(serialized);
  }

  getEntry<T>(key: QueryKey): QueryCacheEntry<T> | undefined {
    return this.cache.get(serializeKey(key)) as QueryCacheEntry<T> | undefined;
  }

  setEntry<T>(key: QueryKey, entry: QueryCacheEntry<T>): void {
    const serialized = serializeKey(key);
    this.cache.set(serialized, entry);
    this.notify(serialized);
  }

  invalidateQueries(filters?: { queryKey?: QueryKey }): void {
    if (!filters?.queryKey) {
      this.cache.forEach((entry) => {
        entry.updatedAt = 0;
      });
      this.notifyAll();
      return;
    }

    const prefix = serializeKey(filters.queryKey).slice(0, -1);
    for (const [key, entry] of this.cache.entries()) {
      if (key.startsWith(prefix)) {
        entry.updatedAt = 0;
        this.notify(key);
      }
    }
  }

  subscribe(key: QueryKey, listener: () => void): () => void {
    const serialized = serializeKey(key);
    if (!this.listeners.has(serialized)) {
      this.listeners.set(serialized, new Set());
    }
    const set = this.listeners.get(serialized)!;
    set.add(listener);

    return () => {
      set.delete(listener);
      if (set.size === 0) {
        this.listeners.delete(serialized);
      }
    };
  }

  subscribeGlobal(listener: () => void): () => void {
    this.globalListeners.add(listener);
    return () => {
      this.globalListeners.delete(listener);
    };
  }

  private notify(serialized: string): void {
    this.listeners.get(serialized)?.forEach((cb) => cb());
    this.globalListeners.forEach((cb) => cb());
  }

  private notifyAll(): void {
    this.listeners.forEach((set) => set.forEach((cb) => cb()));
    this.globalListeners.forEach((cb) => cb());
  }
}

const QueryClientContext = createContext<QueryClient | null>(null);

export const QueryClientProvider: React.FC<{
  client: QueryClient;
  children: React.ReactNode;
}> = ({ client, children }) => {
  return (
    <QueryClientContext.Provider value={client}>
      {children}
    </QueryClientContext.Provider>
  );
};

export function useQueryClient(): QueryClient {
  const client = useContext(QueryClientContext);
  if (!client) {
    throw new Error("useQueryClient must be used within a QueryClientProvider");
  }
  return client;
}

export interface UseQueryOptions<T> {
  queryKey: QueryKey;
  queryFn: () => Promise<T>;
  enabled?: boolean;
  refetchInterval?: number;
  staleTime?: number;
}

export interface UseQueryResult<T> {
  data: T | undefined;
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  refetch: () => Promise<T | undefined>;
}

export function useQuery<T>({
  queryKey,
  queryFn,
  enabled = true,
  refetchInterval,
  staleTime = 0,
}: UseQueryOptions<T>): UseQueryResult<T> {
  const client = useQueryClient();
  const serialized = serializeKey(queryKey);
  const [, setTick] = useState(0);

  const queryFnRef = useRef(queryFn);
  queryFnRef.current = queryFn;

  const fetchData = useCallback(async (): Promise<T | undefined> => {
    const existing = client.getEntry<T>(queryKey);
    client.setEntry(queryKey, {
      data: existing?.data,
      error: null,
      status: "loading",
      updatedAt: existing?.updatedAt || 0,
    });

    try {
      const result = await queryFnRef.current();
      client.setQueryData(queryKey, result);
      return result;
    } catch (err: any) {
      const errorObj = err instanceof Error ? err : new Error(String(err));
      client.setEntry(queryKey, {
        data: existing?.data,
        error: errorObj,
        status: "error",
        updatedAt: Date.now(),
      });
      return undefined;
    }
  }, [client, serialized]);

  useEffect(() => {
    const unsubscribe = client.subscribe(queryKey, () => {
      setTick((t) => t + 1);
    });
    return unsubscribe;
  }, [client, serialized]);

  useEffect(() => {
    if (!enabled) return;

    const entry = client.getEntry<T>(queryKey);
    const isStale = !entry || Date.now() - entry.updatedAt > staleTime;

    if (isStale) {
      fetchData();
    }
  }, [enabled, serialized, staleTime, fetchData]);

  useEffect(() => {
    if (!enabled || !refetchInterval) return;

    const timer = setInterval(() => {
      fetchData();
    }, refetchInterval);

    return () => clearInterval(timer);
  }, [enabled, refetchInterval, fetchData]);

  const entry = client.getEntry<T>(queryKey);

  return {
    data: entry?.data,
    isLoading: entry?.status === "loading" && entry.data === undefined,
    isError: entry?.status === "error",
    error: entry?.error || null,
    refetch: fetchData,
  };
}

export interface UseMutationOptions<TData, TVariables> {
  mutationFn: (variables: TVariables) => Promise<TData>;
  onSuccess?: (data: TData, variables: TVariables) => void | Promise<void>;
  onError?: (error: Error, variables: TVariables) => void | Promise<void>;
}

export interface UseMutationResult<TData, TVariables> {
  mutate: (variables: TVariables) => void;
  mutateAsync: (variables: TVariables) => Promise<TData>;
  isPending: boolean;
  error: Error | null;
  data: TData | undefined;
}

export function useMutation<TData, TVariables>({
  mutationFn,
  onSuccess,
  onError,
}: UseMutationOptions<TData, TVariables>): UseMutationResult<TData, TVariables> {
  const [isPending, setIsPending] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const [data, setData] = useState<TData | undefined>(undefined);

  const mutateAsync = useCallback(
    async (variables: TVariables): Promise<TData> => {
      setIsPending(true);
      setError(null);
      try {
        const result = await mutationFn(variables);
        setData(result);
        if (onSuccess) {
          await onSuccess(result, variables);
        }
        return result;
      } catch (err: any) {
        const errObj = err instanceof Error ? err : new Error(String(err));
        setError(errObj);
        if (onError) {
          await onError(errObj, variables);
        }
        throw errObj;
      } finally {
        setIsPending(false);
      }
    },
    [mutationFn, onSuccess, onError]
  );

  const mutate = useCallback(
    (variables: TVariables) => {
      mutateAsync(variables).catch(() => {});
    },
    [mutateAsync]
  );

  return {
    mutate,
    mutateAsync,
    isPending,
    error,
    data,
  };
}
