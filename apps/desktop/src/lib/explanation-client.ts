import { invoke } from "@tauri-apps/api/core";
import type {
  ExplanationRequest,
  ExplanationResult,
  PreparedExplanation,
  PreparedQuiz,
  QuizResult,
  QuizRequest,
} from "@diffkin/protocol";

export const explanationClient = {
  prepare: (
    comparisonId: string,
    fileId: string,
    eventId: string | null,
    rawStart: number | null,
    jobId: string,
  ): Promise<PreparedExplanation> =>
    invoke("explanation_prepare", { comparisonId, fileId, eventId, rawStart, jobId }),
  run: (request: ExplanationRequest, jobId: string): Promise<ExplanationResult> =>
    invoke("explanation_run", { request, jobId }),
  prepareQuiz: (
    comparisonId: string,
    fileId: string,
    rawStart: number | null,
    jobId: string,
  ): Promise<PreparedQuiz> => invoke("quiz_prepare", { comparisonId, fileId, rawStart, jobId }),
  generateQuiz: (request: QuizRequest, jobId: string): Promise<QuizResult> =>
    invoke("quiz_generate", { request, jobId }),
  cancel: (jobId: string): Promise<void> => invoke("cancel_job", { jobId }),
};
