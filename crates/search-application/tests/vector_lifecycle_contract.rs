//! P2-06: Vector stage, CAS publication, pins, restart and purge.

#[path = "support/api.rs"]
mod api;
#[path = "support/vector.rs"]
mod vector;

use api::ApiWorld;
use search_application::vector::{
    TrustedVectorQuery, VectorBuildOutcome, VectorGenerationPort, VectorIndexPort, VectorLifecycle,
    VectorRecovery, VectorRetriever,
};
use search_core::id::{ResourceId, SourceId};
use search_core::knowledge_unit::KnowledgeUnit;
use search_core::vector::{
    BoundEmbedding, VectorActivationPolicy, VectorManifestInput, VectorStorageKind,
};
use time::OffsetDateTime;
use uuid::Uuid;
use vector::*;

const SCOPE: &str = "tenant-a-scope";

struct World {
    api: ApiWorld,
    provider: Provider,
    index: Index,
    generations: Generations,
    resolver: Resolver,
}

impl World {
    async fn new() -> Self {
        Self {
            api: ApiWorld::new().await,
            provider: Provider::new(spec("commit-abc123")),
            index: Index::default(),
            generations: Generations::default(),
            resolver: Resolver::default(),
        }
    }

    fn source(&self) -> SourceId {
        self.api.document
    }

    fn lifecycle_with<'a>(&'a self, provider: &'a Provider) -> VectorLifecycle<'a> {
        VectorLifecycle {
            provider,
            index: &self.index,
            generations: &self.generations,
        }
    }

    fn lifecycle(&self) -> VectorLifecycle<'_> {
        self.lifecycle_with(&self.provider)
    }

    async fn build(
        &self,
        input: &VectorManifestInput,
        previous: &[BoundEmbedding],
    ) -> VectorBuildOutcome {
        self.lifecycle()
            .build(
                input,
                previous,
                VectorStorageKind::Persistent,
                OffsetDateTime::now_utc(),
            )
            .await
            .unwrap()
    }

    /// The pinned manifest digest for one P1 key, if any.
    async fn pinned(&self, generation_number: u128) -> Option<String> {
        self.generations
            .pin_current(
                generation(self.source(), generation_number),
                &self.provider.spec.validate_and_id().unwrap(),
            )
            .await
            .unwrap()
            .map(|pin| pin.manifest().manifest_digest.clone())
    }

    async fn search(&self, generation_number: u128, text: &str) -> (Vec<ResourceId>, bool) {
        let scope = scope(&self.api).await;
        let query = TrustedVectorQuery::compile(
            &scope,
            VectorActivationPolicy::EligibleOptIn,
            &self.provider.spec,
            text,
            10,
        )
        .unwrap();
        let batch = VectorRetriever {
            provider: &self.provider,
            index: &self.index,
            generations: &self.generations,
            resolver: &self.resolver,
        }
        .retrieve(
            generation(self.source(), generation_number),
            &query,
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();
        (
            batch
                .candidates()
                .iter()
                .map(|candidate| candidate.resource_ref.unwrap())
                .collect(),
            batch.is_unavailable(),
        )
    }
}

fn rid(value: u128) -> ResourceId {
    ResourceId::from_uuid(Uuid::from_u128(value))
}

fn units(source: SourceId) -> Vec<KnowledgeUnit> {
    vec![
        unit(source, 1, 0, "alpha", 9),
        unit(source, 2, 0, "beta", 9),
        unit(source, 3, 0, "gamma", 9),
    ]
}

/// Embeddings the index holds for one published generation.
async fn embeddings(
    world: &World,
    generation_number: u128,
    input: &VectorManifestInput,
) -> Vec<BoundEmbedding> {
    let _ = generation_number;
    input
        .units
        .iter()
        .map(|item| {
            BoundEmbedding::new(
                &world.provider.spec,
                &item.unit,
                &item.authority,
                vector_of(&item.unit.text),
            )
            .unwrap()
        })
        .collect()
}

#[tokio::test]
async fn stage_failure_and_cas_loss_preserve_old_pointer() {
    let world = World::new().await;
    let source = world.source();
    let first = input(&units(source), generation(source, 1), SCOPE);
    world.generations.set_p1(generation(source, 1));
    world.resolver.current(&first);
    assert!(matches!(
        world.build(&first, &[]).await,
        VectorBuildOutcome::Published(_)
    ));
    let published = world.pinned(1).await.unwrap();
    // A partial stage write fails two-way validation: discarded, old kept.
    world.generations.set_p1(generation(source, 2));
    let second = input(&units(source), generation(source, 2), SCOPE);
    *world.index.drop_one.lock().unwrap() = true;
    assert_eq!(
        world.build(&second, &[]).await,
        VectorBuildOutcome::Rejected
    );
    assert_eq!(world.index.stage_count(), 1);
    assert_eq!(world.pinned(1).await.as_deref(), Some(published.as_str()));
    assert_eq!(world.pinned(2).await, None);
    // The P1 bundle moves on before publication: CAS lost, stage discarded.
    world.generations.set_p1(generation(source, 3));
    assert_eq!(world.build(&second, &[]).await, VectorBuildOutcome::LostCas);
    assert_eq!(world.index.stage_count(), 1);
    assert_eq!(world.pinned(1).await.as_deref(), Some(published.as_str()));
    assert_eq!(world.pinned(2).await, None);
}

#[tokio::test]
async fn queries_pin_same_p1_vector_key() {
    let world = World::new().await;
    let source = world.source();
    let first = input(&units(source), generation(source, 1), SCOPE);
    world.generations.set_p1(generation(source, 1));
    world.resolver.current(&first);
    assert!(matches!(
        world.build(&first, &[]).await,
        VectorBuildOutcome::Published(_)
    ));
    assert_eq!(
        world.search(1, "alpha").await,
        (vec![rid(1), rid(2), rid(3)], false)
    );
    // P1 moved to generation 2 without its Vector generation: the query
    // pinned to generation 2 never reads generation 1's vectors.
    world.generations.set_p1(generation(source, 2));
    assert_eq!(world.search(2, "alpha").await, (vec![], true));
    // An in-flight evaluation pinned to generation 1 still reads it.
    assert_eq!(world.search(1, "alpha").await.0[0], rid(1));
}

#[tokio::test]
async fn restart_orphan_unready() {
    let world = World::new().await;
    let source = world.source();
    let first = input(&units(source), generation(source, 1), SCOPE);
    world.generations.set_p1(generation(source, 1));
    world.resolver.current(&first);
    assert!(matches!(
        world.build(&first, &[]).await,
        VectorBuildOutcome::Published(_)
    ));
    // A crash between stage and publication leaves an orphan stage.
    let model = world.provider.spec.validate_and_id().unwrap();
    world
        .index
        .stage(
            generation(source, 2),
            &model,
            &embeddings(&world, 2, &first).await,
        )
        .await
        .unwrap();
    assert_eq!(world.index.stage_count(), 2);
    // Restart: the published generation revalidates, the orphan is gone
    // and is never pinnable.
    let recovery = world
        .lifecycle()
        .recover(std::slice::from_ref(&first))
        .await
        .unwrap();
    assert_eq!(
        recovery,
        VectorRecovery {
            kept: 1,
            withdrawn: 0,
            orphans_discarded: 1
        }
    );
    assert_eq!(world.index.stage_count(), 1);
    assert!(world.pinned(1).await.is_some());
    // Without its P1 input, a published generation is not inferred READY.
    let recovery = world.lifecycle().recover(&[]).await.unwrap();
    assert_eq!(recovery.withdrawn, 1);
    assert_eq!(world.pinned(1).await, None);
    assert_eq!(world.search(1, "alpha").await, (vec![], true));
}

#[tokio::test]
async fn full_incremental_add_change_delete_equivalent() {
    let world = World::new().await;
    let source = world.source();
    let before = input(&units(source), generation(source, 1), SCOPE);
    world.generations.set_p1(generation(source, 1));
    world.resolver.current(&before);
    assert!(matches!(
        world.build(&before, &[]).await,
        VectorBuildOutcome::Published(_)
    ));
    let previous = embeddings(&world, 1, &before).await;
    // Next snapshot: unit 1 unchanged, 2 changed, 3 deleted, 4 added.
    let after_units = vec![
        unit(source, 1, 0, "alpha", 9),
        unit(source, 2, 0, "beta changed", 9),
        unit(source, 4, 0, "gamma new", 9),
    ];
    let after = input(&after_units, generation(source, 2), SCOPE);
    world.generations.set_p1(generation(source, 2));
    let embedded_before = world.provider.embedded();
    let VectorBuildOutcome::Published(incremental) = world.build(&after, &previous).await else {
        panic!("incremental build must publish");
    };
    // Only the changed and added Units were embedded again.
    assert_eq!(world.provider.embedded() - embedded_before, 2);
    let full_provider = Provider::new(spec("commit-abc123"));
    let full_world = World::new().await;
    full_world.generations.set_p1(generation(source, 2));
    let VectorBuildOutcome::Published(full) = full_world
        .lifecycle_with(&full_provider)
        .build(
            &after,
            &[],
            VectorStorageKind::Persistent,
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap()
    else {
        panic!("full build must publish");
    };
    assert_eq!(full_provider.embedded(), 3);
    // The same canonical Unit set gives the same entries and counts.
    assert_eq!(incremental.indexed_count, full.indexed_count);
    let incremental_manifest = world
        .generations
        .published()
        .await
        .unwrap()
        .into_iter()
        .find(|manifest| manifest.bundle_key == generation(source, 2))
        .unwrap();
    let full_manifest = full_world.generations.published().await.unwrap().remove(0);
    assert_eq!(
        incremental_manifest.indexed_entries_digest,
        full_manifest.indexed_entries_digest
    );
    assert_eq!(
        incremental_manifest.unit_bindings_digest,
        full_manifest.unit_bindings_digest
    );
}

#[tokio::test]
async fn model_profile_change_reembeds() {
    let world = World::new().await;
    let source = world.source();
    let first = input(&units(source), generation(source, 1), SCOPE);
    world.generations.set_p1(generation(source, 1));
    assert!(matches!(
        world.build(&first, &[]).await,
        VectorBuildOutcome::Published(_)
    ));
    let previous = embeddings(&world, 1, &first).await;
    // Another model: nothing of the old model is reused.
    let other = Provider::new(spec("commit-other"));
    world.generations.set_p1(generation(source, 2));
    let second = input(&units(source), generation(source, 2), SCOPE);
    assert!(matches!(
        world
            .lifecycle_with(&other)
            .build(
                &second,
                &previous,
                VectorStorageKind::Persistent,
                OffsetDateTime::now_utc()
            )
            .await
            .unwrap(),
        VectorBuildOutcome::Published(_)
    ));
    assert_eq!(other.embedded(), 3);
    // Another extraction profile: every Unit is embedded again.
    world.generations.set_p1(generation(source, 3));
    let reprofiled: Vec<KnowledgeUnit> = vec![
        unit(source, 1, 0, "alpha", 29),
        unit(source, 2, 0, "beta", 29),
        unit(source, 3, 0, "gamma", 29),
    ];
    let third = input(&reprofiled, generation(source, 3), SCOPE);
    let embedded_before = world.provider.embedded();
    assert!(matches!(
        world.build(&third, &previous).await,
        VectorBuildOutcome::Published(_)
    ));
    assert_eq!(world.provider.embedded() - embedded_before, 3);
}

#[tokio::test]
async fn reused_bytes_rebound_after_permission() {
    let world = World::new().await;
    let source = world.source();
    let first = input(&units(source), generation(source, 1), SCOPE);
    world.generations.set_p1(generation(source, 1));
    assert!(matches!(
        world.build(&first, &[]).await,
        VectorBuildOutcome::Published(_)
    ));
    let previous = embeddings(&world, 1, &first).await;
    // Same authority scope and lease in the next generation: bytes are
    // reused but re-bound to the new generation.
    world.generations.set_p1(generation(source, 2));
    let second = input(&units(source), generation(source, 2), SCOPE);
    let embedded_before = world.provider.embedded();
    assert!(matches!(
        world.build(&second, &previous).await,
        VectorBuildOutcome::Published(_)
    ));
    assert_eq!(world.provider.embedded(), embedded_before);
    let manifest = world
        .generations
        .published()
        .await
        .unwrap()
        .into_iter()
        .find(|manifest| manifest.bundle_key == generation(source, 2))
        .unwrap();
    for entry in world.index.staged_entries(&manifest.index).await.unwrap() {
        assert_eq!(entry.hit.generation, generation(source, 2));
    }
    // A changed permission scope never reuses the old bytes.
    world.generations.set_p1(generation(source, 3));
    let rescoped = input(
        &units(source),
        generation(source, 3),
        "tenant-a-other-scope",
    );
    let embedded_before = world.provider.embedded();
    assert!(matches!(
        world.build(&rescoped, &previous).await,
        VectorBuildOutcome::Published(_)
    ));
    assert_eq!(world.provider.embedded() - embedded_before, 3);
}

#[tokio::test]
async fn revocation_expiry_scope_change_and_cancel_purge() {
    let world = World::new().await;
    let source = world.source();
    let first = input(&units(source), generation(source, 1), SCOPE);
    world.generations.set_p1(generation(source, 1));
    world.resolver.current(&first);
    assert!(matches!(
        world.build(&first, &[]).await,
        VectorBuildOutcome::Published(_)
    ));
    // Revocation or scope change of the authority scope purges everything.
    assert_eq!(world.lifecycle().purge(SCOPE).await.unwrap(), 3);
    assert_eq!(world.pinned(1).await, None);
    assert_eq!(world.index.entry_count(), 0);
    assert_eq!(world.search(1, "alpha").await, (vec![], true));
    // An expired lease makes a pinned generation unusable for queries.
    let mut expiring = input(&units(source), generation(source, 2), SCOPE);
    let expiry = OffsetDateTime::now_utc() + time::Duration::milliseconds(200);
    expiring.lease_expires_at = Some(expiry);
    for item in &mut expiring.units {
        item.authority.lease_expires_at = Some(expiry);
    }
    world.generations.set_p1(generation(source, 2));
    world.resolver.current(&expiring);
    assert!(matches!(
        world.build(&expiring, &[]).await,
        VectorBuildOutcome::Published(_)
    ));
    assert!(!world.search(2, "alpha").await.1);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert_eq!(world.search(2, "alpha").await, (vec![], true));
    // A build cancelled after staging leaves only an orphan the next
    // recovery discards.
    let model = world.provider.spec.validate_and_id().unwrap();
    let stages = world.index.stage_count();
    world
        .index
        .stage(
            generation(source, 3),
            &model,
            &embeddings(&world, 3, &first).await,
        )
        .await
        .unwrap();
    let recovery = world
        .lifecycle()
        .recover(&[expiring.clone()])
        .await
        .unwrap();
    // The purged generation's emptied stage and the cancelled stage.
    assert_eq!(stages, 2);
    assert_eq!(recovery.orphans_discarded, 2);
    assert_eq!(world.index.stage_count(), 1);
}
