export type ProviderName = "claude" | "codex";

export const PROVIDERS: { value: ProviderName; label: string; canTrash: boolean }[] = [
  { value: "claude", label: "Claude", canTrash: true },
  // Codex maintains a separate session index; moving rollout files alone leaves stale entries.
  { value: "codex", label: "Codex", canTrash: false },
];

export function providerLabel(provider: string): string {
  return PROVIDERS.find((item) => item.value === provider)?.label ?? provider;
}

export function ptySessionId(provider: string, id: string): string {
  return `${provider}:${id}`;
}

export function providerCanTrash(provider: string): boolean {
  return PROVIDERS.find((item) => item.value === provider)?.canTrash ?? false;
}
