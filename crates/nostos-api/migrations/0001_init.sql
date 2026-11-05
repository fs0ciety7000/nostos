-- Nostos, step 1: trips, photos and the stops derived from them.
create extension if not exists postgis;

create table trips (
    id          uuid primary key default gen_random_uuid(),
    slug        text not null unique check (slug ~ '^[a-z0-9]+(-[a-z0-9]+)*$'),
    title       text not null,
    created_at  timestamptz not null default now()
);

-- Escales: places where photos cluster in space and time. Recomputed from the
-- photos after each import; a name given by hand survives recomputation.
create table stops (
    id           uuid primary key default gen_random_uuid(),
    trip_id      uuid not null references trips on delete cascade,
    seq          integer not null,
    name         text,
    geom         geography(Point, 4326) not null,
    arrived_at   timestamptz not null,
    left_at      timestamptz not null,
    photo_count  integer not null,
    -- Fewer than 3 photos in a short time: a photo taken on the way.
    is_waypoint  boolean not null default false,
    unique (trip_id, seq)
);
create index stops_trip on stops (trip_id, seq);

create table photos (
    id              uuid primary key default gen_random_uuid(),
    trip_id         uuid not null references trips on delete cascade,
    file_name       text not null,
    byte_size       bigint not null check (byte_size > 0),
    original_key    text not null unique,
    status          text not null default 'uploading'
                    check (status in ('uploading', 'pending', 'processing', 'ready', 'failed')),
    error           text,
    attempts        smallint not null default 0,

    container       text,
    camera          text,
    lens            text,
    captured_at     timestamptz,        -- absolute instant when the offset is known
    captured_local  timestamp,          -- camera wall clock, always kept
    utc_offset_min  smallint,
    f_number        real,
    exposure_time   text,
    iso             integer,
    focal_mm        real,
    focal_35mm      smallint,
    width           integer,
    height          integer,
    film_simulation text,
    recipe          jsonb,

    geom            geography(Point, 4326),
    altitude_m      real,
    gps_source      text check (gps_source in ('exif', 'gpx', 'manual')),
    stop_id         uuid references stops on delete set null,

    thumb_key       text,
    preview_key     text,
    thumbhash       bytea,
    palette         text[],

    created_at      timestamptz not null default now(),
    updated_at      timestamptz not null default now()
);
create index photos_trip_time on photos (trip_id, captured_at);
create index photos_queue on photos (created_at) where status = 'pending';
create index photos_geom on photos using gist (geom);
