# Localitysrv

Localitysrv is an HTTP server built with Rust and Axum that serves vector tiles (pmtiles) for localities (cities, towns, villages) worldwide. It extracts and serves pmtiles files based on geographic boundaries from the WhosOnFirst admin database, using either a local planet pmtiles file or the latest Protomaps planet tiles as the source. The server runs simultaneously as both a regular HTTP server on localhost and a Tor hidden service for enhanced privacy.

## Features

- **Vector Tile Serving**: Serves pmtiles (vector tiles) for localities worldwide
- **Geographic Boundaries**: Extracts tiles based on precise geographic boundaries
- **RESTful API**: Provides clean API endpoints for countries and localities
- **Dual Mode Operation**: Runs simultaneously as both a regular HTTP server on localhost and a Tor hidden service
- **Pagination Support**: Efficient pagination for large datasets
- **Search Functionality**: Search countries and localities by name
- **Range Request Support**: HTTP 206 Partial Content support for efficient tile loading
- **Concurrent Processing**: Configurable concurrency for extraction tasks
- **Multiple Data Sources**: Support for both local and remote planet pmtiles files

## Architecture

### Core Components

- **API Layer** (`src/api/`): HTTP endpoint handlers

  - `countries.rs`: Country listing and filtering
  - `localities.rs`: Locality search and pagination
  - `pmtiles.rs`: Pmtiles file serving with range request support

- **Services Layer** (`src/services/`): Business logic

  - `country.rs`: Country data management and filtering
  - `database.rs`: SQLite database operations with optimized indexes
  - `extraction.rs`: Pmtiles extraction from local or remote planet tiles

- **Models** (`src/models/`): Data structures

  - `country.rs`: Country information
  - `locality.rs`: Locality data with geographic boundaries
  - `response.rs`: API response structures

- **Utilities** (`src/utils/`): Helper functions

  - `cmd.rs`: Command-line tool execution
  - `file.rs`: File operations and downloads

- **Configuration** (`src/config.rs`): Environment variable management
- **Initialization** (`src/initialization.rs`): First-run setup and data management

### Flow

1. Server startup → Configuration loading → Database initialization
2. Dual server startup: localhost HTTP server and Tor hidden service
3. API requests → Service layer → Database/External operations
4. Pmtiles extraction (from local file or remote source) → File storage → HTTP serving

## Installation

### Prerequisites

- Rust (latest stable version)
- Cargo (included with Rust)
- `pmtiles` command-line tool
- `bzip2` command-line tool
- `find` command-line tool

### Building from Source

```bash
git clone https://github.com/yourusername/localitysrv.git
cd localitysrv
cargo build --release
```

### Running the Server

```bash
cargo run
```

Or using the release build:

```bash
./target/release/localitysrv
```

## Command Line Options

The server supports several command line options to control initialization behavior, particularly useful for automated deployments and non-interactive environments:

### Options

- `--non-interactive, -n`: Enable non-interactive mode (automatically downloads and extracts)
- `--no-download`: Skip downloading the database if missing
- `--no-extract`: Skip extracting missing localities
- `--help, -h`: Show help message
- `--version, -v`: Show version information

### Usage Examples

```bash
# Fully automatic mode (download and extract without prompting)
cargo run -- --non-interactive
# or
./target/release/localitysrv -n

# Skip downloading database if missing (exit with error if database is missing)
cargo run -- --no-download

# Skip extracting localities if missing (continue with available data)
cargo run -- --no-extract

# Show help
cargo run -- --help
# or
./target/release/localitysrv -h
```

## Configuration

The server is configured through environment variables. Create a `.env` file in the project root:

```env
# Server Configuration
SERVER_PORT=8000
ASSETS_DIR=./assets

# Command-line Tool Paths
PMTILES_CMD=pmtiles
BZIP2_CMD=bzip2
FIND_CMD=find

# Database Configuration
WHOSEONFIRST_DB_URL=https://data.geocode.earth/wof/dist/sqlite/whosonfirst-data-admin-latest.db.bz2

# Protomaps Configuration
PROTOMAPS_BUILDS_URL=https://build-metadata.protomaps.dev/builds.json

# Local Planet PMTiles Configuration
# Set this to the path of a local planet.pmtiles file to use it instead of downloading from remote
# Example: PLANET_PMTILES_PATH=/path/to/planet.pmtiles
# If not set or empty, the system will fetch the latest planet pmtiles from the remote URL
PLANET_PMTILES_PATH=

# Target Countries (comma-separated, empty or ALL for all countries)
TARGET_COUNTRIES=AE,AF

# Performance Settings
MAX_CONCURRENT_EXTRACTIONS=10
DB_CONNECTION_POOL_SIZE=10
```

### Configuration Options

- `SERVER_PORT`: Port for the HTTP server (default: 8000)
- `ASSETS_DIR`: Directory for storing assets (default: ./assets)
- `PMTILES_CMD`: Path to the pmtiles command-line tool (default: pmtiles)
- `BZIP2_CMD`: Path to the bzip2 command-line tool (default: bzip2)
- `FIND_CMD`: Path to the find command-line tool (default: find)
- `WHOSEONFIRST_DB_URL`: URL for the WhosOnFirst database (default: latest from data.geocode.earth)
- `PROTOMAPS_BUILDS_URL`: URL for Protomaps builds metadata (default: build-metadata.protomaps.dev)
- `PLANET_PMTILES_PATH`: Optional path to a local planet.pmtiles file
- `TARGET_COUNTRIES`: Comma-separated list of country codes to process (empty for all countries)
- `MAX_CONCURRENT_EXTRACTIONS`: Maximum concurrent extraction tasks (default: 10)
- `DB_CONNECTION_POOL_SIZE`: Database connection pool size (default: 10)

## API Endpoints

### Health Check

```
GET /health
```

Returns the health status of the server.

**Response:**

```json
{
  "status": "healthy"
}
```

### Countries

```
GET /countries?page={page}&limit={limit}&q={query}
```

Retrieves a paginated list of countries with optional search.

**Parameters:**

- `page`: Page number (default: 1)
- `limit`: Items per page (default: 10)
- `q`: Search query (optional)

**Response:**

```json
{
  "success": true,
  "data": [
    {
      "country_code": "AE",
      "country_name": "United Arab Emirates",
      "locality_count": 42
    }
  ],
  "pagination": {
    "total": 1,
    "page": 1,
    "limit": 10,
    "total_pages": 1
  }
}
```

### Localities

```
GET /countries/{country_code}/localities?page={page}&limit={limit}&q={query}
```

Retrieves a paginated list of localities for a specific country with optional search.

**Parameters:**

- `country_code`: ISO country code
- `page`: Page number (default: 1)
- `limit`: Items per page (default: 10)
- `q`: Search query (optional)

**Response:**

```json
{
  "success": true,
  "data": [
    {
      "id": 85632721,
      "name": "Abu Dhabi",
      "country": "AE",
      "placetype": "locality",
      "latitude": 24.4764,
      "longitude": 54.3457,
      "min_longitude": 54.244,
      "min_latitude": 24.331,
      "max_longitude": 54.511,
      "max_latitude": 24.545,
      "file_size": 1024,
      "onion_link": "http://example.onion/countries/AE/localities/85632721/pmtiles"
    }
  ],
  "pagination": {
    "total": 1,
    "page": 1,
    "limit": 10,
    "total_pages": 1
  }
}
```

### PMTiles

```
GET /countries/{country_code}/localities/{id}/pmtiles
```

Serves the pmtiles file for a specific locality. Supports HTTP range requests for efficient loading.

**Parameters:**

- `country_code`: ISO country code
- `id`: Locality ID

**Response:**
The pmtiles file content with appropriate headers:

- `Content-Type: application/octet-stream`
- `Content-Length: {file_size}`
- `Accept-Ranges: bytes`
- `Content-Disposition: attachment; filename="{id}.pmtiles"`

For range requests, returns HTTP 206 Partial Content with:

- `Content-Range: bytes {start}-{end}/{total_size}`

## Data Sources

### WhosOnFirst Database

The server uses the WhosOnFirst admin database as its primary source for locality data. This database contains comprehensive information about places worldwide, including:

- Geographic coordinates and boundaries
- Place types and hierarchies
- Current and deprecated status
- Country associations

The database is automatically downloaded and initialized on first run.

### Protomaps Planet Tiles

Vector tiles are extracted from planet-scale pmtiles files. The server supports two sources:

1. **Remote Source**: Fetches the latest planet pmtiles from Protomaps builds
2. **Local Source**: Uses a local planet.pmtiles file if specified in configuration

Using a local planet pmtiles file is recommended for better performance and reduced bandwidth usage.

## Localhost server + hidden service

The server runs simultaneously in two modes:

1. **Local HTTP Server**: Accessible on localhost at the configured port
2. **Tor Hidden Service**: Accessible through the Tor network via an onion address

### Local HTTP Server

The local HTTP server starts automatically when the application launches. By default, it listens on port 8000, but this can be configured via the `SERVER_PORT` environment variable.

```
✓ TCP listener binded to http://127.0.0.1:8000
```

You can access all API endpoints locally:

```
http://127.0.0.1:8000/countries
http://127.0.0.1:8000/countries/AE/localities
http://127.0.0.1:8000/countries/AE/localities/85632721/pmtiles
```

### Tor Hidden Service

The Tor hidden service powered by Arti also starts automatically when the application launches. It creates an onion address that can be used to access the service through the Tor network for enhanced privacy.

The onion address will be displayed in the logs:

```
✓ Tor hidden service is now fully reachable at http://example123.onion
```

You can access all API endpoints through the Tor hidden service:

```
http://example123.onion/countries
http://example123.onion/countries/AE/localities
http://example123.onion/countries/AE/localities/85632721/pmtiles
```

### Performance Tuning

For better performance:

1. Use a local planet pmtiles file via `PLANET_PMTILES_PATH`
2. Adjust `MAX_CONCURRENT_EXTRACTIONS` based on your system capabilities
3. Optimize `DB_CONNECTION_POOL_SIZE` for your workload

## License

Licensed under GNU GPL v3+

## Acknowledgments

- [WhosOnFirst](https://whosonfirst.org/) for the comprehensive place database
- [Protomaps](https://protomaps.com/) for the vector tile infrastructure
- [OpenStreetMap](https://www.openstreetmap.org/) for the actual map data
- [Arti](https://gitlab.torproject.org/tpo/core/arti/) for the Tor implementation
