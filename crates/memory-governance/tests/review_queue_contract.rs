use memory_governance::{ReviewMemoryEnvelope, ReviewQueue, ReviewStatus};

#[test]
fn review_queue_contract_includes_only_memories_requiring_review() {
    let queue = ReviewQueue::from_memory_envelopes([
        memory(ReviewFixture::new("quarantined", "Quarantined contradiction", "quarantined").review("quarantined")),
        memory(ReviewFixture::new("candidate", "Needs confirmation", "candidate").requires_confirmation()),
        memory(ReviewFixture::new("pending", "Pending review", "active").review("pending")),
        memory(ReviewFixture::new("active", "Active memory", "active")),
        memory(ReviewFixture::new("pinned", "Pinned memory", "pinned")),
        memory(ReviewFixture::new("superseded", "Superseded memory", "superseded")),
        memory(ReviewFixture::new("archived", "Archived memory", "archived")),
        memory(ReviewFixture::new("tombstoned", "Tombstoned memory", "tombstoned")),
    ]);

    let ids: Vec<_> = queue.items.iter().map(|item| item.id.as_str()).collect();
    assert_eq!(ids, ["quarantined", "candidate", "pending"]);

    let quarantined = &queue.items[0];
    assert_eq!(quarantined.summary, "Quarantined contradiction");
    assert_eq!(quarantined.status, ReviewStatus::Quarantined);
    assert_eq!(quarantined.policy_applied, "governance-test-policy");
    assert_eq!(quarantined.reason.as_deref(), Some("contradicts pinned guidance"));
    assert_eq!(quarantined.next_actions, ["review_approve", "review_reject"]);

    let candidate = &queue.items[1];
    assert_eq!(candidate.status, ReviewStatus::Candidate);
    assert!(candidate.reason.as_deref().expect("candidate reason").contains("confirmation"));

    let pending = &queue.items[2];
    assert_eq!(pending.status, ReviewStatus::PendingReview);
    assert_eq!(pending.reason.as_deref(), Some("awaiting human review"));
}

#[test]
fn review_status_serializes_as_stable_snake_case() {
    assert_eq!(ReviewStatus::PendingReview.as_str(), "pending_review");
    assert_eq!(serde_json::to_value(ReviewStatus::PendingReview).unwrap(), "pending_review");
    assert_eq!(serde_json::from_str::<ReviewStatus>("\"pending-review\"").unwrap(), ReviewStatus::PendingReview);
}

fn memory(fixture: ReviewFixture<'_>) -> ReviewMemoryEnvelope {
    ReviewMemoryEnvelope {
        id: fixture.id.to_string(),
        summary: fixture.summary.to_string(),
        status: fixture.status.to_string(),
        requires_user_confirmation: fixture.requires_user_confirmation,
        review_state: fixture.review_state.map(str::to_string),
        policy_applied: "governance-test-policy".to_string(),
        reason: Some(review_reason(fixture.status, fixture.review_state).to_string()),
    }
}

struct ReviewFixture<'a> {
    id: &'a str,
    summary: &'a str,
    status: &'a str,
    requires_user_confirmation: bool,
    review_state: Option<&'a str>,
}

impl<'a> ReviewFixture<'a> {
    fn new(id: &'a str, summary: &'a str, status: &'a str) -> Self {
        Self { id, summary, status, requires_user_confirmation: false, review_state: None }
    }

    fn requires_confirmation(mut self) -> Self {
        self.requires_user_confirmation = true;
        self
    }

    fn review(mut self, review_state: &'a str) -> Self {
        self.review_state = Some(review_state);
        self
    }
}

fn review_reason(status: &str, review_state: Option<&str>) -> &'static str {
    match (status, review_state) {
        ("quarantined", _) => "contradicts pinned guidance",
        (_, Some("pending")) => "awaiting human review",
        _ => "requires user confirmation",
    }
}
