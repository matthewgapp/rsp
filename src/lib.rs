// use rsb_derive::Builder;
use serde::Serialize;
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Location<ID, C> {
    id: Option<ID>,
    txn_id: Option<u32>,
    collection: C,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct UpdatableResource<ID, T, C>
where
    T: TS,
{
    location: Location<ID, C>,
    data: T,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct AppendableResource<ID, T, C>
where
    T: TS,
{
    location: Location<ID, C>,
    data: T,
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct ResourceId(u32);

#[derive(Debug, Clone, Serialize, TS)]
// this produces a json object with a "type" field and a "payload" field
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
#[ts(export)]
pub enum EventVerb<ID, T: Serialize + TS, C> {
    Insert(AppendableResource<ID, T, C>),
    Update(UpdatableResource<ID, T, C>),
    Upsert(UpdatableResource<ID, T, C>),
    Delete(ResourceId),
}

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct Event<ID, T: Serialize + TS, C> {
    verb: EventVerb<ID, T, C>,
}

impl<ID, T: Serialize + TS, C> Event<ID, T, C> {
    pub fn new(verb: EventVerb<ID, T, C>) -> Self {
        Self { verb }
    }

    pub fn new_insert_event(data: T, collection: C) -> Self {
        let location = Location {
            id: None,
            txn_id: None,
            collection,
        };
        let verb = EventVerb::Insert(AppendableResource { location, data });
        Self::new(verb)
    }

    pub fn into_ws_body(self) -> WsBody<Self>
    where
        Self: Serialize,
    {
        WsBody::new(self)
    }
}

#[derive(Serialize)]
pub struct WsBody<T: Serialize> {
    data: T,
}

impl<T: Serialize> WsBody<T> {
    fn new(data: T) -> Self {
        Self { data }
    }

    // TODO: this should return a result type
    pub fn json(&self) -> String {
        serde_json::to_string(&self).expect("Could not serialize WsBody<T> to JSON")
    }
}

impl<T> From<T> for WsBody<T>
where
    T: Serialize,
{
    fn from(data: T) -> Self {
        Self::new(data)
    }
}

pub trait Appendable: Serialize + Sized + TS {
    type Collection;

    fn collection(&self) -> Self::Collection;

    fn to_insert_event(self) -> Event<(), Self, Self::Collection> {
        <Self as Appendable>::to_event(self, EventVerb::Insert)
    }

    fn to_event(
        self,
        build_verb: impl FnOnce(
            AppendableResource<(), Self, Self::Collection>,
        ) -> EventVerb<(), Self, Self::Collection>,
    ) -> Event<(), Self, Self::Collection> {
        let location = Location {
            id: None,
            txn_id: None,
            collection: self.collection(),
        };
        let verb = build_verb(AppendableResource {
            location,
            data: self,
        });
        Event::new(verb)
    }
}

pub trait Syncable: Appendable {
    type Id;

    fn id(&self) -> Self::Id;

    fn to_upsert_event(self) -> Event<Self::Id, Self, Self::Collection> {
        <Self as Syncable>::to_event(self, EventVerb::Upsert)
    }

    fn to_update_event(self) -> Event<Self::Id, Self, Self::Collection> {
        <Self as Syncable>::to_event(self, EventVerb::Update)
    }

    fn to_event(
        self,
        build_verb: impl FnOnce(
            UpdatableResource<Self::Id, Self, Self::Collection>,
        ) -> EventVerb<Self::Id, Self, Self::Collection>,
    ) -> Event<Self::Id, Self, Self::Collection> {
        let location = Location {
            id: Some(self.id()),
            txn_id: None,
            collection: self.collection(),
        };
        let verb = build_verb(UpdatableResource {
            location,
            data: self,
        });
        Event::new(verb)
    }
}

mod test {
    use serde::Serialize;
    use ts_rs::TS;

    use crate::{Appendable, Syncable};

    #[derive(Serialize, TS)]
    struct DoggoRecord {
        id: u32,
        name: String,
        breed: String,
    }

    #[allow(dead_code)]
    #[derive(Serialize, TS)]
    enum Collection {
        Dogs,
        Cats,
    }

    impl Appendable for DoggoRecord {
        type Collection = Collection;

        fn collection(&self) -> Self::Collection {
            Collection::Dogs
        }
    }

    impl Syncable for DoggoRecord {
        type Id = u32;

        fn id(&self) -> u32 {
            self.id
        }
    }

    #[test]
    fn conversion_works() {
        use super::*;

        let doggo = DoggoRecord {
            id: 1,
            name: "Barky".to_string(),
            breed: "Poodle".to_string(),
        };

        let event = doggo.to_insert_event();
        let json = WsBody::new(event).json();
        insta::assert_snapshot!(json, @r###"{"data":{"verb":{"type":"insert","payload":{"location":{"id":null,"txn_id":null,"collection":"Dogs"},"data":{"id":1,"name":"Barky","breed":"Poodle"}}}}}"###);

        let doggo = DoggoRecord {
            id: 1,
            name: "Barky".to_string(),
            breed: "Poodle".to_string(),
        };
        let event = doggo.to_upsert_event();
        let json = WsBody::new(event).json();

        insta::assert_snapshot!(json, @r###"{"data":{"verb":{"type":"upsert","payload":{"location":{"id":1,"txn_id":null,"collection":"Dogs"},"data":{"id":1,"name":"Barky","breed":"Poodle"}}}}}"###);
    }
}

#[async_trait::async_trait]
pub trait Listener {
    type Error;
    type Item;

    async fn recv(&mut self) -> Result<Self::Item, Self::Error>;
}

pub trait Service<T> {
    type Listener: Listener<Item = T>;
    type Error;

    fn publish(&self, event: T) -> Result<(), Self::Error>;
    fn listener(&self) -> Self::Listener;
}
