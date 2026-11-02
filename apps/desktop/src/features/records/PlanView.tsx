import { useEffect, useState, type FormEvent } from "react";
import { useTranslation } from "react-i18next";

import { toIpcError, type IpcClient } from "../../ipc/client";
import type {
  IpcError,
  PlanDto,
  RecordDto,
  RecordKind,
  RecordStatus,
} from "../../ipc/types";
import { ALL_STATUSES, linesToItems, nextStatuses } from "./recordState";

export type PlanClient = Pick<
  IpcClient,
  "planGet" | "recordSetStatus" | "recordEdit" | "recordCreate"
>;

type PlanViewProps = {
  sessionId: string;
  client: PlanClient;
  /** Changing it reloads the plan (e.g. after new proposals arrive). */
  refreshKey?: number;
};

const KINDS: readonly RecordKind[] = [
  "task",
  "requirement",
  "decision",
  "assumption",
  "risk",
  "open_question",
];

type EditorProps = {
  record: RecordDto;
  busy: boolean;
  onSave: (fields: {
    text: string;
    rationale: string;
    acceptanceCriteria?: string[];
    alternatives?: string[];
  }) => void;
  onCancel: () => void;
};

function ItemEditor({ record, busy, onSave, onCancel }: EditorProps) {
  const { t } = useTranslation();
  const [text, setText] = useState(record.text);
  const [rationale, setRationale] = useState(record.rationale);
  const [criteria, setCriteria] = useState(
    record.acceptanceCriteria.join("\n"),
  );
  const [alternatives, setAlternatives] = useState(
    record.alternatives.join("\n"),
  );

  function submit(event: FormEvent) {
    event.preventDefault();
    onSave({
      text,
      rationale,
      acceptanceCriteria:
        record.kind === "task" ? linesToItems(criteria) : undefined,
      alternatives:
        record.kind === "decision" ? linesToItems(alternatives) : undefined,
    });
  }

  return (
    <form
      className="plan-editor"
      onSubmit={submit}
      onKeyDown={(event) => {
        if (event.key === "Escape") onCancel();
      }}
    >
      <label>
        {t("plan.text")}
        <textarea
          value={text}
          required
          maxLength={2000}
          autoFocus
          onChange={(e) => setText(e.target.value)}
        />
      </label>
      <label>
        {t("plan.rationale")}
        <textarea
          value={rationale}
          maxLength={2000}
          onChange={(e) => setRationale(e.target.value)}
        />
      </label>
      {record.kind === "task" && (
        <label>
          {t("plan.criteriaHint")}
          <textarea
            value={criteria}
            onChange={(e) => setCriteria(e.target.value)}
          />
        </label>
      )}
      {record.kind === "decision" && (
        <label>
          {t("plan.alternativesHint")}
          <textarea
            value={alternatives}
            onChange={(e) => setAlternatives(e.target.value)}
          />
        </label>
      )}
      <button type="submit" disabled={busy || text.trim() === ""}>
        {t("plan.save")}
      </button>
      <button type="button" onClick={onCancel}>
        {t("plan.cancel")}
      </button>
    </form>
  );
}

function NewItemForm({
  busy,
  onCreate,
}: {
  busy: boolean;
  onCreate: (kind: RecordKind, text: string, status: RecordStatus) => void;
}) {
  const { t } = useTranslation();
  const [kind, setKind] = useState<RecordKind>("task");
  const [status, setStatus] = useState<RecordStatus>("confirmed");
  const [text, setText] = useState("");

  function submit(event: FormEvent) {
    event.preventDefault();
    if (text.trim() === "") return;
    onCreate(kind, text, status);
    setText("");
  }

  return (
    <form
      className="plan-new"
      onSubmit={submit}
      aria-labelledby="plan-new-title"
    >
      <h3 id="plan-new-title">{t("plan.addTitle")}</h3>
      <label>
        {t("plan.kind")}
        <select
          value={kind}
          onChange={(e) => setKind(e.target.value as RecordKind)}
        >
          {KINDS.map((k) => (
            <option key={k} value={k}>
              {t(`records.kind.${k}`)}
            </option>
          ))}
        </select>
      </label>
      <label>
        {t("plan.text")}
        <textarea
          value={text}
          required
          maxLength={2000}
          onChange={(e) => setText(e.target.value)}
        />
      </label>
      <label>
        {t("plan.initialStatus")}
        <select
          value={status}
          onChange={(e) => setStatus(e.target.value as RecordStatus)}
        >
          {ALL_STATUSES.map((s) => (
            <option key={s} value={s}>
              {t(`records.status.${s}`)}
            </option>
          ))}
        </select>
      </label>
      <button type="submit" disabled={busy || text.trim() === ""}>
        {t("plan.add")}
      </button>
    </form>
  );
}

/**
 * Development plan of a session (US4, FR-031/FR-032): sections per kind as the core orders
 * them, a status filter, inline editing and manual items. Every change goes through the core,
 * which keeps the history; the view only reloads what the core returns.
 */
export function PlanView({ sessionId, client, refreshKey = 0 }: PlanViewProps) {
  const { t } = useTranslation();
  const [plan, setPlan] = useState<PlanDto | null>(null);
  const [visible, setVisible] = useState<ReadonlySet<RecordStatus>>(
    new Set(ALL_STATUSES),
  );
  const [editing, setEditing] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<IpcError | null>(null);
  /** Bumped after each change so the plan is read back from the core. */
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let active = true;
    client.planGet(sessionId).then(
      (loaded) => active && setPlan(loaded),
      (failure: unknown) => active && setError(toIpcError(failure)),
    );
    return () => {
      active = false;
    };
  }, [client, sessionId, refreshKey, version]);

  async function run(action: () => Promise<unknown>) {
    setBusy(true);
    setError(null);
    try {
      await action();
      setEditing(null);
      setVersion((v) => v + 1);
    } catch (failure) {
      setError(toIpcError(failure));
    } finally {
      setBusy(false);
    }
  }

  function toggle(status: RecordStatus) {
    setVisible((current) => {
      const next = new Set(current);
      if (next.has(status)) next.delete(status);
      else next.add(status);
      return next;
    });
  }

  function renderItem(record: RecordDto) {
    if (editing === record.id) {
      return (
        <ItemEditor
          record={record}
          busy={busy}
          onCancel={() => setEditing(null)}
          onSave={(fields) =>
            void run(() =>
              client.recordEdit({ recordId: record.id, ...fields }),
            )
          }
        />
      );
    }
    return (
      <article className={`plan-item record-${record.status}`}>
        <p className="record-text">{record.text}</p>
        <ul className="record-badges" aria-label={t("records.badgesLabel")}>
          <li className="badge badge-status">
            {t(`records.status.${record.status}`)}
          </li>
          <li className="badge">{t(`records.nature.${record.nature}`)}</li>
          {record.origin === "user" && (
            <li className="badge">{t("records.userCreated")}</li>
          )}
          {record.conflictsWith && (
            <li className="badge badge-warning">{t("records.conflict")}</li>
          )}
          {record.duplicateOf && (
            <li className="badge badge-warning">{t("records.duplicate")}</li>
          )}
        </ul>
        {record.rationale && (
          <p className="record-rationale">
            <strong>{t("records.rationale")}</strong> {record.rationale}
          </p>
        )}
        {record.acceptanceCriteria.length > 0 && (
          <>
            <h4>{t("records.acceptanceCriteria")}</h4>
            <ul>
              {record.acceptanceCriteria.map((item, i) => (
                <li key={i}>{item}</li>
              ))}
            </ul>
          </>
        )}
        {record.alternatives.length > 0 && (
          <>
            <h4>{t("records.alternatives")}</h4>
            <ul>
              {record.alternatives.map((item, i) => (
                <li key={i}>{item}</li>
              ))}
            </ul>
          </>
        )}
        <div
          role="group"
          className="record-actions"
          aria-label={t("records.actionsLabel", { text: record.text })}
        >
          {nextStatuses(record.status).map((status) => (
            <button
              key={status}
              type="button"
              disabled={busy}
              onClick={() =>
                void run(() =>
                  client.recordSetStatus({ recordId: record.id, status }),
                )
              }
            >
              {t(`records.action.${status}`)}
            </button>
          ))}
          <button
            type="button"
            disabled={busy}
            onClick={() => setEditing(record.id)}
          >
            {t("plan.edit")}
          </button>
        </div>
      </article>
    );
  }

  return (
    <section className="plan" aria-labelledby="plan-title">
      <h2 id="plan-title">{t("plan.title")}</h2>
      {error && <p role="alert">{t(error.messageKey)}</p>}
      <fieldset className="plan-filter">
        <legend>{t("plan.filter")}</legend>
        {ALL_STATUSES.map((status) => (
          <label key={status}>
            <input
              type="checkbox"
              checked={visible.has(status)}
              onChange={() => toggle(status)}
            />
            {t(`records.status.${status}`)}
          </label>
        ))}
      </fieldset>
      {!plan
        ? !error && <p>{t("app.loading")}</p>
        : plan.sections.map((section) => {
            const items = section.items.filter((r) => visible.has(r.status));
            const headingId = `plan-${section.kind}`;
            return (
              <section key={section.kind} aria-labelledby={headingId}>
                <h3 id={headingId}>
                  {t(`plan.section.${section.kind}`)} ({items.length})
                </h3>
                {items.length === 0 ? (
                  <p className="plan-empty">{t("plan.sectionEmpty")}</p>
                ) : (
                  <ul className="plan-items">
                    {items.map((record) => (
                      <li key={record.id}>{renderItem(record)}</li>
                    ))}
                  </ul>
                )}
              </section>
            );
          })}
      <NewItemForm
        busy={busy}
        onCreate={(kind, text, status) =>
          void run(() => client.recordCreate({ sessionId, kind, text, status }))
        }
      />
    </section>
  );
}
