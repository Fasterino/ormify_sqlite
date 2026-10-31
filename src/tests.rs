use ormify::{
    DefaultLanguageEnum, Driver, Entity, FieldType, Joined, Observer, Ormify, Supported, Transform,
    UnixTimeStamp, Uuid, VarChar,
};
use serde::{Deserialize, Serialize};

use crate::SQLite;

struct Printer;

impl Supported<SQLite> for Printer {
    fn get_language() -> Self::LanguageEnum {
        Default::default()
    }
}

impl Driver for Printer {
    type LanguageEnum = DefaultLanguageEnum;

    type ConnectArgs = ();

    fn connect(
        _args: Self::ConnectArgs,
        _language: Self::LanguageEnum,
    ) -> impl std::future::Future<Output = ormify::Result<Self>> {
        async move {
            println!("Connected!");
            Ok(Self)
        }
    }

    fn query(
        &mut self,
        query: ormify::LanguageQuery,
    ) -> impl std::future::Future<Output = ormify::Result<Vec<ormify::Entry>>> {
        async move {
            println!("{:?}\n", query);
            Ok(if query.query.contains("users") {
                vec![User::builder().build().to_entry()]
            } else if query.query.contains("posts") {
                vec![Post::builder().build().to_entry()]
            } else {
                vec![]
            })
        }
    }

    fn execute(
        &mut self,
        commands: Vec<ormify::LanguageQuery>,
    ) -> impl std::future::Future<Output = ormify::Result<ormify::ExecutionResponse>> {
        async move {
            for query in commands {
                println!("{:?}\n", query);
            }
            Ok(Default::default())
        }
    }
}

const NAME: &'static str = "DumbModule";
// mod test;

#[derive(Entity, Debug)]
#[db(group=NAME)]
#[db(rename = "users")]
struct User {
    #[db(pk)]
    pub id: Uuid,
    #[db(unique)]
    pub nickname: VarChar<32>,
    pub created_at: UnixTimeStamp,
    #[db(update=UnixTimeStamp::new)]
    pub modified_at: UnixTimeStamp,
    #[db(index)]
    pub age: Option<i32>,
    #[db(rename = "description")]
    pub bio: std::option::Option<VarChar<255>>,
    pub a: TestStruct,
    pub b: TestEnum,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, FieldType)]
#[ft(Vec<u8>)]
struct TestStruct {}

#[derive(Clone, Default, Debug, FieldType)]
#[ft(i32)]
enum TestEnum {
    #[default]
    None,
    Field1,
    Field2,
}

#[derive(Entity, Debug)]
#[db(group=NAME)]
#[db(rename = "posts")]
struct Post {
    #[db(pk)]
    pub id: Uuid,
    #[db(fk: mega_fk_name -> id in User)]
    pub author_id: Uuid,
    pub created_at: UnixTimeStamp,
    #[db(update=Default::default)]
    pub modified_at: UnixTimeStamp,
    pub content: VarChar<255>,
}

#[tokio::test]
async fn test_init_query() -> ormify::Result<()> {
    let mut ormify = Ormify::<SQLite, Printer>::connect(()).await?;

    ormify.execute(vec![Post::init(), User::init()]).await?;

    let user = User::builder().b(TestEnum::Field2).build();

    let post = Post::builder().author_id(user.id).build();

    ormify.execute(vec![user.insert(), post.insert()]).await?;

    let req = User::select().join::<Post>(|fk, s| (fk.mega_fk_name, s));
    let res = ormify.query(req).await?;

    let user_with_posts: Joined<Observer<User>, Observer<Post>> = Transform::transform(res)?;

    let user = user_with_posts.entity;
    let posts = user_with_posts.children;

    println!("User: {:?}\nPosts: {:?}", user, posts);

    Ok(())
}
