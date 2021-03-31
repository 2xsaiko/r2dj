CREATE TABLE IF NOT EXISTS __migtool_meta
(
    id     uuid,
    ta     serial,
    run_at timestamp DEFAULT timezone('utc', now()),

    PRIMARY KEY (id)
);