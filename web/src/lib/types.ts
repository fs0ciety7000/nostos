// Shapes returned by nostos-api (see crates/nostos-api/src/routes.rs).

export interface TripSummary {
	id: string;
	slug: string;
	title: string;
	photos: number;
	starts_on: string | null;
	ends_on: string | null;
}

export interface Stop {
	id: string;
	seq: number;
	name: string | null;
	lat: number;
	lon: number;
	arrived_at: string;
	left_at: string;
	photos: number;
	photos_with_gps: number;
	is_waypoint: boolean;
	film_simulation: string | null;
	cover_url: string | null;
}

export interface TripDetail {
	trip: { id: string; slug: string; title: string; created_at: string };
	photos: { ready: number; in_flight: number; failed: number; with_gps: number };
	film_simulations: { name: string; photos: number }[];
	days: { chant: number; date: string; photos: number }[];
	stops: Stop[];
	route_km: number | null;
}

export interface Recipe {
	film_simulation?: unknown;
	grain_roughness?: 'Off' | 'Weak' | 'Strong' | null;
	grain_size?: 'Off' | 'Small' | 'Large' | null;
	color_chrome?: 'Off' | 'Weak' | 'Strong' | null;
	color_chrome_fx_blue?: 'Off' | 'Weak' | 'Strong' | null;
	shadow_tone?: number | null;
	highlight_tone?: number | null;
	color?: number | null;
	clarity?: number | null;
	white_balance?: unknown;
	wb_shift?: { red_raw: number; blue_raw: number } | null;
	dynamic_range?: 'Auto' | { Percent: number } | null;
}

export interface Photo {
	id: string;
	file_name: string;
	camera: string | null;
	lens: string | null;
	captured_at: string | null;
	captured_local: string | null;
	f_number: number | null;
	exposure_time: string | null;
	iso: number | null;
	focal_mm: number | null;
	focal_35mm: number | null;
	width: number | null;
	height: number | null;
	film_simulation: string | null;
	recipe: Recipe | null;
	lat: number | null;
	lon: number | null;
	stop_id: string | null;
	thumb_url: string | null;
	preview_url: string | null;
	thumbhash: string | null;
	palette: string[] | null;
}
