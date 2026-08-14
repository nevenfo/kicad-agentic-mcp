use kam_context::{
    BudgetLimits, BudgetState, Compactor, ContextBudget, RetrievalBundle, TaskCore,
    TokenCountedMessage,
};
use kam_llm::{Message, Role, Usage};
use kam_state::TaskState;
use serde_json::Value;

#[derive(Clone, Copy)]
struct RecordedCall {
    input: u32,
    output: u32,
    reasoning: u32,
}

// E27, model_decoupling_bank/full, gpt-oss-20b medium, observed window 32_768.
const FULL: [RecordedCall; 5] = [
    RecordedCall {
        input: 2_538,
        output: 1_688,
        reasoning: 1_484,
    },
    RecordedCall {
        input: 2_542,
        output: 1_164,
        reasoning: 1_018,
    },
    RecordedCall {
        input: 2_538,
        output: 1_424,
        reasoning: 1_276,
    },
    RecordedCall {
        input: 2_542,
        output: 1_486,
        reasoning: 1_328,
    },
    RecordedCall {
        input: 2_542,
        output: 1_227,
        reasoning: 1_026,
    },
];
const NONE_INPUT: [u32; 5] = [2_383, 2_387, 2_383, 2_391, 2_383];
const FIXED_PREFIX_TOKENS: u32 = 2_015;

#[test]
fn recorded_local_calls_survive_a_complete_budget_and_compaction_cycle() {
    let limits = BudgetLimits::new(32_768, 5_120).unwrap();
    let mut budget = ContextBudget::new(limits);
    for call in FULL {
        let snapshot = budget
            .record(Usage {
                prompt_tokens: call.input,
                completion_tokens: call.output,
                reasoning_tokens: call.reasoning,
                ..Usage::default()
            })
            .unwrap();
        assert_eq!(snapshot.state, BudgetState::WithinBudget);
        assert_eq!(
            snapshot.answer_tokens + snapshot.reasoning_tokens,
            snapshot.completion_tokens
        );
    }

    let mut task = TaskState::new(
        "model_decoupling_bank",
        "Place C10 through C13 as four grounded 100nF capacitors on +3V3.",
    );
    task.add_constraint("references are exactly C10, C11, C12, C13")
        .unwrap();
    task.add_constraint("ERC errors must not exceed the measured baseline of 2")
        .unwrap();
    task.add_verified_fact("E27 full arm completed this fixture at grade 3 in 5/5 calls");
    task.add_verified_fact("Device:C pin offsets are -3.81mm and +3.81mm on y");

    // `none` measures the dynamic task+anchor without the full retrieval hint.
    // The largest per-attempt full-minus-none delta measures that hint's cost.
    let dynamic_without_retrieval = NONE_INPUT
        .into_iter()
        .map(|input| input - FIXED_PREFIX_TOKENS)
        .max()
        .unwrap();
    let retrieval_tokens = FULL
        .into_iter()
        .zip(NONE_INPUT)
        .map(|(with, without)| with.input - without)
        .max()
        .unwrap();

    let retrieval = [RetrievalBundle::new(
        "model_decoupling_bank/full",
        "electrical: isolated +3V3 bank expects ERC baseline 2; \
         Plan IR: put one power symbol on each capacitor pin; \
         geometry: Device:C pins are y-3.81 and y+3.81",
        retrieval_tokens,
    )];

    // Replay measured completions until the recorded session exceeds the
    // prompt budget. The compactor must evict only an oldest transcript prefix.
    let transcript = (0..4)
        .flat_map(|round| {
            FULL.into_iter()
                .enumerate()
                .map(move |(index, call)| (round, index, call))
        })
        .map(|(round, index, call)| TokenCountedMessage {
            message: Message::text(
                Role::Assistant,
                format!("E27 plan round {round}, call {index}"),
            ),
            measured_tokens: call.output,
        })
        .collect::<Vec<_>>();

    let compacted = Compactor::new(limits)
        .compact_with_retrieval(
            TaskCore::from_task(&task, dynamic_without_retrieval),
            FIXED_PREFIX_TOKENS,
            &retrieval,
            &transcript,
        )
        .unwrap();

    assert!(compacted.used_prompt_tokens() <= u64::from(limits.prompt_capacity_tokens()));
    assert!(compacted.dropped_messages() > 0);
    assert!(!compacted.messages().is_empty());
    assert_eq!(compacted.retrieval()[0].id(), "model_decoupling_bank/full");

    let core: Value = serde_json::from_str(compacted.task_core().rendered()).unwrap();
    assert_eq!(core["active_task"]["objective"], task.objective);
    assert_eq!(
        core["active_task"]["constraints"],
        serde_json::json!(&task.constraints)
    );
    assert_eq!(
        core["active_task"]["verified_facts"],
        serde_json::json!(&task.verified_facts)
    );
}
