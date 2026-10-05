-- Apply with the database owner after crates/search-runtime/sql/roles.sql and
-- Graph migration 0001. Capability roles only; deployment LOGIN principals are
-- granted these roles. Row locks (FOR UPDATE/SHARE) need a column UPDATE grant.
GRANT USAGE ON SCHEMA search_graph TO search_registration, search_builder,
    search_coordinator, search_reader, search_gc;

GRANT SELECT ON ALL TABLES IN SCHEMA search_graph
    TO search_builder, search_coordinator, search_reader, search_gc;
GRANT SELECT ON search_graph.generation, search_graph.build_guard TO search_registration;

-- Parents and incremental guards are registered only inside the P7 transaction.
GRANT INSERT ON search_graph.generation, search_graph.build_guard
    TO search_registration, search_coordinator;

-- Builders write children of a BUILDING parent under its live guard.
GRANT INSERT, DELETE ON search_graph.resource, search_graph.relation,
    search_graph.participant TO search_builder;
GRANT UPDATE (batch_phase, batch_sequence) ON search_graph.generation TO search_builder;
GRANT UPDATE (copy_verified_at) ON search_graph.build_guard TO search_builder;

-- The coordinator settles builds and renews or removes guards.
GRANT UPDATE (state, graph_content_digest, resource_count, relation_count, ready_at)
    ON search_graph.generation TO search_coordinator;
GRANT UPDATE (expires_at) ON search_graph.build_guard TO search_coordinator;
GRANT DELETE ON search_graph.build_guard TO search_coordinator;

-- Only GC retires: DELETING, then children, then the parent.
GRANT UPDATE (state) ON search_graph.generation TO search_gc;
GRANT DELETE ON ALL TABLES IN SCHEMA search_graph TO search_gc;
