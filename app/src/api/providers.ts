// AI providers and their models (spec §6.1, UI-SET-2). Rust emits DataChanged { providers } after
// every change, so mutations here don't invalidate anything themselves. Errors from mutations are
// shown as toasts (client.ts).
//
// Query keys: ['providers', 'list'] · ['providers', 'models']
//
// API keys are write-only: `setProviderKey` sends one to the OS keychain and nothing ever returns it.
import { useMutation, useQuery } from '@tanstack/react-query'
import {
  type AzureSignin,
  commands,
  type ModelConfig,
  type ModelPatch,
  type NewModel,
  type NewProvider,
  type Provider,
  type ProviderPatch,
  type RemoteModel,
  type TestReport,
} from './bindings'
import { queryClient } from './client'

export type {
  AuthMode,
  AzureSignin,
  EffortConfig,
  ModelConfig,
  ModelPatch,
  NewModel,
  NewProvider,
  Provider,
  ProviderKind,
  ProviderPatch,
  RemoteModel,
  RequestFormat,
  TestCheck,
  TestReport,
} from './bindings'

const EMPTY_PROVIDERS: Provider[] = []
const EMPTY_MODELS: ModelConfig[] = []

const providersKey = ['providers', 'list'] as const
const modelsKey = ['providers', 'models'] as const

/** Levels the chat picker offers when a model doesn't list its own (FR-LLM-EF-4). Same as Rust. */
export const DEFAULT_EFFORT_LEVELS = ['low', 'medium', 'high']

// ------------------------------------------------------------------ reading

/** Providers in card order, with `hasKey` filled in. */
export function useProviders() {
  return useQuery({ queryKey: providersKey, queryFn: () => commands.listProviders() })
}

/** Every model of every provider, in provider then model order. */
export function useModels() {
  return useQuery({ queryKey: modelsKey, queryFn: () => commands.listModels() })
}

/** For screens that only look things up: empty until loaded, never a new array. */
export function useProviderList(): Provider[] {
  return useProviders().data ?? EMPTY_PROVIDERS
}

export function useModelList(): ModelConfig[] {
  return useModels().data ?? EMPTY_MODELS
}

/** Models of providers that are switched on (FR-LLM-PV-6): what the pickers offer. */
export function useUsableModels(): ModelConfig[] {
  const providers = useProviderList()
  const models = useModelList()
  return usableModels(providers, models)
}

export function usableModels(providers: Provider[], models: ModelConfig[]): ModelConfig[] {
  const on = new Set(providers.filter((p) => p.enabled).map((p) => p.id))
  return models.filter((m) => on.has(m.providerId))
}

/** Cached lists for code outside React (the fake assistant). Empty if not loaded yet. */
export function getModelsSnapshot(): ModelConfig[] {
  return queryClient.getQueryData<ModelConfig[]>(modelsKey) ?? EMPTY_MODELS
}

export function getProvidersSnapshot(): Provider[] {
  return queryClient.getQueryData<Provider[]>(providersKey) ?? EMPTY_PROVIDERS
}

/** The effort levels a model offers in chat (FR-LLM-EF-4). Empty when effort is off. */
export function effortLevels(model: ModelConfig | undefined): string[] {
  if (!model?.effort.enabled) return []
  return model.effort.levels ?? DEFAULT_EFFORT_LEVELS
}

// ------------------------------------------------------------------ Azure endpoint style

export type AzureStyle = 'openai_v1' | 'anthropic' | 'classic'

/**
 * The Azure endpoint style from the URL. Mirrors `azure_style` in `cairn-core/src/providers.rs`;
 * the UI uses it to show the detected style and to hide the api-version field.
 */
export function azureStyle(endpoint: string): AzureStyle {
  const rest = endpoint.trim().toLowerCase().replace(/^[a-z]+:\/\//, '')
  const path = (rest.split(/[?#]/)[0] ?? '').replace(/\/+$/, '')
  if (path.includes('/openai/v1')) return 'openai_v1'
  if (path.endsWith('/anthropic') || path.includes('/anthropic/')) return 'anthropic'
  return 'classic'
}

// ------------------------------------------------------------------ changing

export function useCreateProvider() {
  return useMutation({ mutationFn: (input: NewProvider) => commands.createProvider(input) })
}

export function useUpdateProvider() {
  return useMutation({ mutationFn: ({ id, patch }: { id: string; patch: ProviderPatch }) => commands.updateProvider(id, patch) })
}

export function useDeleteProvider() {
  return useMutation({ mutationFn: (id: string) => commands.deleteProvider(id) })
}

export function useSetProviderKey() {
  return useMutation({ mutationFn: ({ id, key }: { id: string; key: string }) => commands.setProviderKey(id, key) })
}

export function useRemoveProviderKey() {
  return useMutation({ mutationFn: (id: string) => commands.removeProviderKey(id) })
}

export function useCreateModel() {
  return useMutation({ mutationFn: (input: NewModel) => commands.createModel(input) })
}

export function useUpdateModel() {
  return useMutation({ mutationFn: ({ id, patch }: { id: string; patch: ModelPatch }) => commands.updateModel(id, patch) })
}

export function useDeleteModel() {
  return useMutation({ mutationFn: (id: string) => commands.deleteModel(id) })
}

// ------------------------------------------------------------------ calls to the provider (through the sidecar)

/** *Test connection* for one model: basic call, streaming, each effort level, image. */
export function testModel(modelId: string): Promise<TestReport> {
  return commands.testModel(modelId)
}

/** *Fetch models*: what the provider's API lists. Azure providers can't (the error says so). */
export function fetchModels(providerId: string): Promise<RemoteModel[]> {
  return commands.fetchModels(providerId)
}

/** *Check sign-in* for the Azure CLI (`az login`). */
export function checkAzureSignin(): Promise<AzureSignin> {
  return commands.checkAzureSignin()
}
