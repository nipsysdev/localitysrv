# localitysrv - pmtiles server and Tor hidden service for serving world localities

localitysrv is an HTTP server built with Rust and Axum, which can also be started as a Tor .onion service, that serves pmtiles (vector tiles) for localities (cities, towns, villages) from around the world.\
It
extracts and serves pmtiles files based on geographic boundaries from the
WhosOnFirst admin database, using the latest Protomaps planet tiles as the source.

## Features

- **Automatic Data Management**: Downloads and manages the WhosOnFirst database
- **On-demand Tile Extraction**: Extracts pmtiles for specific localities using bounding boxes
- **RESTful API**: Provides endpoints for browsing countries and localities
- **Concurrent Processing**: Supports concurrent extraction tasks for improved performance
- **Range Request Support**: Supports HTTP range requests for efficient pmtiles file serving
- **Search Functionality**: Search localities by name with pagination support
- **Country Filtering**: Limit processing to specific countries if needed
- **Local Planet PMTiles Support**: Use a local planet pmtiles file instead of downloading from remote
- **Tor Hidden Service Support**: Run as a Tor hidden service for enhanced privacy thanks to Arti

## Prerequisites

Before running localitysrv, ensure you have the following command-line tools
installed:

- `pmtiles` - For extracting pmtiles files
  ([Install instructions here](https://docs.protomaps.com/guide/getting-started))
- `bzip2` - For decompressing the database
- `find` - For efficient file operations

## Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/nipsysdev/localitysrv.git
   cd localitysrv
   ```

2. Install Rust if you haven't already:\
   https://www.rust-lang.org/tools/install

3. Build the project:
   ```bash
   cargo build --release
   ```

## Usage

### Starting the Server

Run the following command to start the server:

```bash
cargo run
```

The server will start on port 8080 (or as configured in `.env` file).

### First Run

On the first run, the application will:

1. Check that required command-line tools are available
2. Download the WhosOnFirst admin database (compressed bzip2) - with your confirmation
3. Decompress the database to SQLite format
4. Check for existing pmtiles files
5. Prompt you to extract missing pmtiles files if needed

> You can also use a local planet pmtiles file instead of downloading from remote by setting
> the `PLANET_PMTILES_PATH` environment variable to the path of your local planet.pmtiles file.

> Be aware that, by default, all localities from every country available will be
> queued for extraction.\
> Downloading everything will take a significant amount of hours (days
> potentially!)
>
> You can restrict the download of pmtiles to specific countries by setting
> TARGET_COUNTRIES in the `.env` file to a comma-separated list of country codes.

## API Endpoints

### Countries

- **GET** `/countries`
  - Returns a list of all countries with their codes and locality counts
  - Response format:
    ```json
    {
      "success": true,
      "data": [
        {
          "countryCode": "BE",
          "countryName": "Belgium",
          "localityCount": 2136
        }
      ]
    }
    ```

### Localities

- **GET** `/countries/{countryCode}/localities`
  - Returns a paginated list of localities for a specific country
  - Query parameters:
    - `q` (optional): Text to search in locality names
    - `page` (optional): Page number (default: 1)
    - `limit` (optional): Number of items per page (default: 20)
  - Response format:
    ```json
    {
      "success": true,
      "data": [
        {
          "id": 101839773,
          "name": "Brugelette",
          "country": "BE",
          "placetype": "locality",
          "latitude": 50.576433,
          "longitude": 3.851975,
          "min_longitude": 3.827,
          "min_latitude": 50.564,
          "max_longitude": 3.877,
          "max_latitude": 50.589,
          "fileSize": 2928590
        }
      ],
      "pagination": {
        "total": 6,
        "page": 1,
        "limit": 20,
        "totalPages": 1
      }
    }
    ```

### Pmtiles Files

- **GET** `/countries/{countryCode}/localities/{id}/pmtiles`
  - Serves the pmtiles file for a specific locality
  - Supports HTTP range requests for partial content
  - Sets appropriate headers for file download
  - Returns 404 if the file doesn't exist

### Health Check

- **GET** `/health`
  - Returns the health status of the server
  - Response format:
    ```json
    {
      "status": "healthy"
    }
    ```

## Configuration

Configuration can be set through environment variables in a `.env` file:

- `SERVER_PORT`: Server port number (default: 8080)
- `ASSETS_DIR`: Directory for storing assets (default: ./assets)
- `PMTILES_CMD`: Command-line tool for pmtiles operations (default: pmtiles)
- `BZIP2_CMD`: Command-line tool for decompression (default: bzip2)
- `FIND_CMD`: Command-line tool for file operations (default: find)
- `WHOSEONFIRST_DB_URL`: URL for the WhosOnFirst database
- `PROTOMAPS_BUILDS_URL`: URL for Protomaps builds JSON list
- `PLANET_PMTILES_PATH`: Path to a local planet pmtiles file (optional, if not set will download from remote)
- `TARGET_COUNTRIES`: Comma-separated list of country codes to process (empty = ALL)
- `MAX_CONCURRENT_EXTRACTIONS`: Maximum number of concurrent extraction tasks (default: 10)
- `DB_CONNECTION_POOL_SIZE`: Database connection pool size (default: 10)
- `TOR_HIDDEN_SERVICE`: Set to "true" to run as a Tor hidden service (default: false)

Example `.env` file:
```
# Server Configuration
SERVER_PORT=8080
ASSETS_DIR=./assets

# Command-line Tool Paths
PMTILES_CMD=pmtiles
BZIP2_CMD=bzip2
FIND_CMD=find

# Database Configuration
WHOSEONFIRST_DB_URL=https://data.geocode.earth/wof/dist/sqlite/whosonfirst-data-admin-latest.db.bz2

# Protomaps Configuration
PROTOMAPS_BUILDS_URL=https://build-metadata.protomaps.dev/builds.json

# Local Planet PMTiles (optional)
# PLANET_PMTILES_PATH=/path/to/your/planet.pmtiles

# Target Countries (comma-separated, empty or ALL for all countries)
TARGET_COUNTRIES=BE,LU

# Performance Settings
MAX_CONCURRENT_EXTRACTIONS=10
DB_CONNECTION_POOL_SIZE=10

# Tor Hidden Service (optional)
# TOR_HIDDEN_SERVICE=false
```

## Architecture

The project is built with the following Rust ecosystem:

- **Axum**: Web framework for building the HTTP server
- **Tokio**: Async runtime for handling concurrent operations
- **Rusqlite**: SQLite database driver with async compatibility layer
- **Reqwest**: HTTP client for downloading files
- **Serde**: JSON serialization/deserialization
- **Thiserror**: Error handling
- **Tower-HTTP**: HTTP middleware including CORS support
- **Futures**: Utilities for async programming
- **Tokio-util**: Additional utilities for Tokio
- **Arti**: Tor client for hidden service functionality

### Project Structure

```
src/
├── api/           # API endpoint handlers
│   ├── countries.rs
│   ├── localities.rs
│   ├── pmtiles.rs
│   └── mod.rs
├── models/        # Data models
│   ├── country.rs
│   ├── locality.rs
│   ├── response.rs
│   └── mod.rs
├── services/      # Business logic services
│   ├── country.rs
│   ├── database.rs
│   ├── extraction.rs
│   └── mod.rs
├── utils/         # Utility functions
│   ├── cmd.rs
│   ├── file.rs
│   └── mod.rs
├── config.rs      # Configuration management
├── initialization.rs # First-run setup
└── main.rs        # Application entry point
```

## Development

### Running Tests

```bash
cargo test
```

### Running in Development Mode

```bash
cargo run
```

### Building for Production

```bash
cargo build --release
```

### Code Quality

Before submitting changes, ensure the code passes linting checks:

```bash
cargo clippy
```

## License

This project is licensed under the GNU GPLv3 License - see the [LICENSE](LICENSE) file for details.