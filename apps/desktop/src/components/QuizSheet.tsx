import { useState } from "react";
import type { ChangeQuiz, QuizEvidence } from "@diffkin/protocol";

const letters = ["A", "B", "C", "D"];

export function QuizSheet({
  quiz,
  evidence,
  generating,
  onGenerate,
}: {
  quiz: ChangeQuiz;
  evidence: QuizEvidence[];
  generating: boolean;
  onGenerate: () => void;
}) {
  const { questions } = quiz;
  const [answers, setAnswers] = useState<(number | null)[]>(() => questions.map(() => null));
  const [page, setPage] = useState(0);
  const [submitted, setSubmitted] = useState(false);
  const question = questions[page];
  function retry() {
    setAnswers(questions.map(() => null));
    setPage(0);
    setSubmitted(false);
  }
  if (submitted) {
    const score = questions.filter((q, i) => q.correctIndex === answers[i]).length;
    return (
      <section aria-label="测验结果" className="quiz-results">
        <h3>
          答对 {score} / {questions.length} 题
        </h3>
        {questions.map((q, i) => {
          const choice = answers[i]!;
          return (
            <article key={q.prompt}>
              <h4>
                第 {i + 1} 题 · {choice === q.correctIndex ? "答对" : "答错"}
              </h4>
              <p>{q.prompt}</p>
              {choice !== q.correctIndex && (
                <p>
                  你的选择 {letters[choice]} · {q.choices[choice]}
                </p>
              )}
              <p>
                <strong>正确答案 {letters[q.correctIndex]}</strong> · {q.choices[q.correctIndex]}
              </p>
              <p>{q.explanation}</p>
              {q.evidenceIds.map((id) => {
                const source = evidence[id - 1];
                return (
                  <details key={id}>
                    <summary>
                      核对源码 · {source.path} · 片段 {id}
                    </summary>
                    <pre>{source.diff}</pre>
                  </details>
                );
              })}
            </article>
          );
        })}
        <div className="explanation-actions">
          <button disabled={generating} onClick={retry}>
            重答这套题
          </button>
          <button disabled={generating} onClick={onGenerate}>
            {generating ? "正在出题…" : "重新出题"}
          </button>
        </div>
      </section>
    );
  }
  return (
    <section className="quiz-sheet" aria-label="答题">
      <p aria-live="polite">
        第 {page + 1} / {questions.length} 题 · 单选
      </p>
      <fieldset key={page}>
        <legend>{question.prompt}</legend>
        {question.choices.map((text, i) => (
          <label key={i} className="quiz-choice">
            <input
              type="radio"
              name="quiz-choice"
              checked={answers[page] === i}
              onChange={() =>
                setAnswers((previous) =>
                  previous.map((value, index) => (index === page ? i : value)),
                )
              }
            />
            <span>
              {letters[i]} · {text}
            </span>
          </label>
        ))}
      </fieldset>
      <div className="explanation-actions">
        <button disabled={page === 0} onClick={() => setPage(page - 1)}>
          上一题
        </button>
        {page + 1 < questions.length ? (
          <button
            className="primary"
            disabled={answers[page] === null}
            onClick={() => setPage(page + 1)}
          >
            下一题
          </button>
        ) : (
          <button
            className="primary"
            disabled={answers.some((choice) => choice === null)}
            onClick={() => setSubmitted(true)}
          >
            交卷评分
          </button>
        )}
      </div>
    </section>
  );
}
