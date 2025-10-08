# Torality - Pmtiles Server for World Localities

Torality is an HTTP server built with Rust and Axum that serves pmtiles (vector
tiles) for localities (cities, towns, villages) from around the world. It
extracts and serves pmtiles files based on geographic boundaries from the
WhosOnFirst admin database.

## Prerequisites

Before running Torality, ensure you have the following command-line tools
installed:

- `pmtiles` - For extracting pmtiles files
  ([Install instructions here](https://docs.protomaps.com/guide/getting-started))
- `bzip2` - For decompressing the database
- `find` - For efficient file operations

## Installation

1. Clone the repository:
   ```bash
   git clone https://github.com/nipsysdev/torality.git
   cd torality
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

> Be aware that, by default, all localities from every country available will be
> queued for extraction.\
> Downloading everything will take a significant amount of hours (days
> potentially!)
>
> You can restrict the download of pmtiles to specific countries by setting
> TARGET_COUNTRIES in the `.env` file to a comma-separated list of country codes.

## API Endpoints

### Countries

- **GET** `/api/countries`
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

- **GET** `/api/countries/:countryCode/localities`
  - Returns a paginated list of 20 localities for a specific country
  - Query parameters:
    - `q` (optional): Text to search in locality names
    - `page` (optional): Page number (default: 1)
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
          "fileSize": 2928590
        },
        {
          "id": 101748097,
          "name": "Brugge",
          "country": "BE",
          "placetype": "locality",
          "latitude": 51.208664,
          "longitude": 3.217718,
          "fileSize": 6773970
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

- **GET** `/api/pmtiles/:countryCode/:localityId`
  - Serves the pmtiles file for a specific locality
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
- `TARGET_COUNTRIES`: Comma-separated list of country codes to process (empty = ALL)
- `MAX_CONCURRENT_EXTRACTIONS`: Maximum number of concurrent extraction tasks (default: 4)
- `DB_CONNECTION_POOL_SIZE`: Database connection pool size (default: 10)

Example `.env` file:
```
SERVER_PORT=8080
ASSETS_DIR=./assets
PMTILES_CMD=pmtiles
BZIP2_CMD=bzip2
FIND_CMD=find
WHOSEONFIRST_DB_URL=https://data.geocode.earth/wof/dist/sqlite/whosonfirst-data-admin-latest.db.bz2
PROTOMAPS_BUILDS_URL=https://build-metadata.protomaps.dev/builds.json
TARGET_COUNTRIES=BE,LU,MQ
MAX_CONCURRENT_EXTRACTIONS=4
DB_CONNECTION_POOL_SIZE=10
```

## Architecture

The project is built with the following Rust ecosystem:

- **Axum**: Web framework for building the HTTP server
- **Tokio**: Async runtime for handling concurrent operations
- **SQLx**: Database driver for SQLite with async support
- **Reqwest**: HTTP client for downloading files
- **Serde**: JSON serialization/deserialization
- **Thiserror**: Error handling

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

## License

This project is licensed under the GNU GPLv3 License.