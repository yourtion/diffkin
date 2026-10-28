import { invoke } from "@tauri-apps/api/core";
import type { RepositoryRefs } from "@diffkin/protocol";

export const navigationClient = {
  pick: (): Promise<string | null> => invoke("pick_repository_directory"),
  refs: (
    repository: string,
    query: string,
    offset: number,
    jobId: string,
  ): Promise<RepositoryRefs> =>
    invoke("list_repository_refs", { repository, query, offset, limit: 100, jobId }),
};
