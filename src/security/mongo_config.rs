// use mongodb::{bson::doc, options::ClientOptions, Client, Collection};
// use serde::{Deserialize, Serialize};
// use std::sync::Arc;
// // use tokio::sync::OnceCell;

// // static MONGO_CLIENT: OnceCell<Arc<Client>> = OnceCell::const_new();

// pub struct MongoDBConnection {
//     client: Arc<Client>,
//     database: String,
// }

// impl MongoDBConnection {
//     pub async fn init(uri: &str, database: &str) -> Result<Self, mongodb::error::Error> {
//         let client_options = ClientOptions::parse(uri).await?;
//         let client = Client::with_options(client_options)?;

//         // Test connection
//         client
//             .database(database)
//             .run_command(doc! {"ping": 1})
//             .await?;

//         // Log success message
//         println!(
//             "✅ Successfully connected to MongoDB database '{}'",
//             database
//         );

//         Ok(Self {
//             client: Arc::new(client),
//             database: database.to_string(),
//         })
//     }

//     fn get_collection(&self) -> Collection<ConnectionStringDoc> {
//         self.client
//             .database(&self.database)
//             .collection("collections")
//     }
// }

// #[derive(Serialize, Deserialize)]
// struct ConnectionStringDoc {
//     #[serde(rename = "connectionString")]
//     connection_string: String,
// }

// pub async fn validate_connection_string_in_mongo(
//     conn: &MongoDBConnection,
//     connection_string: &str,
// ) -> Result<bool, mongodb::error::Error> {
//     let collection = conn.get_collection();
//     let filter = doc! {"connectionString": connection_string};

//     match collection.find_one(filter).await? {
//         Some(doc) => Ok(doc.connection_string == connection_string),
//         None => Ok(false),
//     }
// }
