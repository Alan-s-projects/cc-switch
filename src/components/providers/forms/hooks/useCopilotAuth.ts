import { useManagedAuth } from "./useManagedAuth";

export function useCopilotAuth(githubDomain?: string) {
  return useManagedAuth("github_copilot", githubDomain);
}
