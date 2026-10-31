use std::time::Duration;

use ormify::{
    Benchmark, Entity, FieldType, Joined, Observer, Ordering::*, Ormify, TotalBenchmark, Transform,
    UnixTimeStamp, Uuid, VarChar, c,
};
use ormify_sqlite::{FileEngine, SQLite};
use serde::{Deserialize, Serialize};

const NAME: &'static str = "DumbModule";

#[derive(Entity)]
#[db(group=NAME)]
// #[db(rename = "users")]
pub struct User {
    #[db(pk)]
    pub id: Uuid,
    #[db(unique)]
    pub nickname: VarChar<32>,
    pub created_at: UnixTimeStamp,
    #[db(update=UnixTimeStamp::new)]
    pub modified_at: UnixTimeStamp,
    pub age: Option<i32>,
    #[db(rename = "description")]
    pub bio: std::option::Option<VarChar<255>>,
    pub a: TestStruct,
    pub b: TestEnum,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize, FieldType)]
#[ft(Vec<u8>)]
pub struct TestStruct {}

#[derive(Clone, Default, Debug, FieldType)]
#[ft(i32)]
pub enum TestEnum {
    #[default]
    None,
    Field1,
    Field2,
}

#[derive(Entity)]
#[db(group=NAME)]
// #[db(rename = "posts")]
pub struct Post {
    #[db(pk)]
    pub id: Uuid,
    #[db(fk: author -> id in User)]
    pub author_id: Uuid,
    pub created_at: UnixTimeStamp,
    #[db(update=Default::default)]
    pub modified_at: UnixTimeStamp,
    pub content: VarChar<255>,
}

#[derive(Entity)]
#[db(group=NAME)]
pub struct Comment {
    #[db(pk)]
    pub id: Uuid,
    #[db(fk -> id in Post)] // name will be set automatically
    pub post_id: Uuid,
    #[db(fk: author -> id in User)]
    pub author_id: Uuid,
    pub created_at: UnixTimeStamp,
    #[db(update=Default::default)]
    pub modified_at: UnixTimeStamp,
    pub content: VarChar<255>,
}

fn main() -> ormify::Result {
    // futures::executor::block_on(main_inner())
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Can't build rt")
        .block_on(main_inner())
}

type SQLiteOrmify = Ormify<SQLite, FileEngine>;

pub trait BuildWithTimestamps: Sized {
    type Target: Entity;
    fn build_with_timestamps(self) -> Self::Target;
}
macro_rules! imp_build_with_timestamps {
    ($ident:ident) => {
        impl BuildWithTimestamps for <$ident as Entity>::Builder {
            type Target = $ident;
            fn build_with_timestamps(self) -> Self::Target {
                let timestamp = UnixTimeStamp::new();
                self.created_at(timestamp).modified_at(timestamp).build()
            }
        }
    };
}
imp_build_with_timestamps!(User);
imp_build_with_timestamps!(Post);
imp_build_with_timestamps!(Comment);

async fn main_inner() -> ormify::Result {
    let mut results = Vec::new();
    for i in 0..50 {
        println!("Waiting a second before bench#{}...", i);
        println!();

        std::thread::sleep(Duration::from_secs(1));

        let benchmark = test_run().await?;

        println!("{}", benchmark);
        println!();

        results.push(benchmark);
    }

    let total_benchmark = TotalBenchmark::from(results);
    println!("{}", total_benchmark);

    Ok(())
}

async fn test_run() -> ormify::Result<Benchmark> {
    let args = "./db.sqlite3";

    let mut db = SQLiteOrmify::connect(args).await?;

    println!("Connected!");
    println!();

    let n = db
        .execute(vec![Post::init(), Comment::init(), User::init()])
        .await?;

    println!("Tables inited, row changes: {n}");
    println!();

    let user_id = Uuid::new();
    let user = User::builder()
        .id(user_id)
        .nickname("User 1")
        .build_with_timestamps();

    println!("Inserting user with id {:?}", user_id);
    println!();
    db.execute(vec![user.insert()]).await?;

    let posts: Vec<_> = (0..10).map(|x| format!("Text {}", x)).collect();
    println!(
        "Inserting posts with author_id {:?} and contents {:?}",
        user_id, posts
    );
    println!();
    db.execute(
        posts
            .into_iter()
            .map(|x| {
                Post::builder()
                    .author_id(user_id)
                    .content(x)
                    .build_with_timestamps()
                    .insert()
            })
            .collect(),
    )
    .await?;

    println!("Inserting second user with automatically set id");
    println!();
    let second_user = User::builder().nickname("User 2").build_with_timestamps();
    let second_user_id = second_user.id;
    db.execute(vec![second_user.insert()]).await?;

    println!("Selecting user");
    println!();
    let res = db
        .query(User::select().where_(|x| c!(x.id == user_id)).limit(1))
        .await?;
    let user = Observer::<User>::transform(res)?;

    println!("Selecting user's posts");
    println!();
    let res = db
        .query(
            user.select_children::<Post>(|fk| fk.author)?
                .order_by(|x| vec![x.created_at(Ascending)]),
        )
        .await?;
    let posts_ids: Vec<Uuid> = Vec::<Post>::transform(res)?
        .into_iter()
        .map(|x| x.id)
        .collect();

    let new_user_id = Uuid::new();
    println!(
        "Changing user with id {:?}: Replacing id to {:?}",
        user_id, new_user_id
    );
    println!();
    set_user_id(&mut db, user, new_user_id).await?;

    println!("Creating comments");
    println!();
    for author_id in [new_user_id, second_user_id] {
        for post_id in posts_ids.iter().copied() {
            for i in 0..10 {
                let content = format!("Text {}", i);

                db.execute(vec![
                    Comment::builder()
                        .author_id(author_id)
                        .post_id(post_id)
                        .content(content)
                        .build_with_timestamps()
                        .insert(),
                ])
                .await?;
            }
        }
    }

    check_number_of_rows::<User>(&mut db).await?;
    check_number_of_rows::<Post>(&mut db).await?;
    check_number_of_rows::<Comment>(&mut db).await?;
    println!();

    println!("Getting all users with their posts");
    println!();

    let res = db
        .query(
            User::select()
                .order_by(|x| vec![x.nickname(Ascending)])
                .join::<Comment>(|fk, s| (fk.author, s))
                .join::<Post>(|fk, s| (fk.author, s.join::<Comment>(|fk, s| (fk.post, s)))),
        )
        .await?;
    let users_with_posts_and_comments = Transform::transform(res)?;

    delete_users(&mut db, users_with_posts_and_comments).await?;

    check_number_of_rows::<User>(&mut db).await?;
    check_number_of_rows::<Post>(&mut db).await?;
    check_number_of_rows::<Comment>(&mut db).await?;
    println!();

    Ok(db.get_benchmark())
}

type UserWithPostsAndComments = Joined<Observer<User>, (Observer<Comment>, PostWithComments)>;
type PostWithComments = Joined<Observer<Post>, Observer<Comment>>;

async fn delete_users(
    db: &mut SQLiteOrmify,
    users_with_posts: Vec<UserWithPostsAndComments>,
) -> ormify::Result {
    for Joined {
        entity: user,
        children: (comments, posts_with_comments),
    } in users_with_posts
    {
        println!("Deleting user with id: {:?}", user.id);
        println!("Nickname: {:?}", user.nickname);
        println!(
            "Number of linked posts: {} (Total posts comments: {})",
            posts_with_comments.len(),
            posts_with_comments
                .iter()
                .map(
                    |Joined {
                         children: comments, ..
                     }| comments.len()
                )
                .sum::<usize>()
        );
        println!("Number of linked comments: {}", comments.len());
        println!(
            "Was modified (created_at != modified_at): {}",
            user.created_at != user.modified_at
        );
        println!();
        db.execute(vec![user.delete()]).await?;
    }

    Ok(())
}

async fn set_user_id(db: &mut SQLiteOrmify, mut user: Observer<User>, id: Uuid) -> ormify::Result {
    user.id = id;
    db.execute(vec![user.update()]).await?;
    Ok(())
}

async fn check_number_of_rows<T: Entity>(db: &mut SQLiteOrmify) -> ormify::Result {
    let table_name = T::scheme().name;

    let res = db.query(T::select()).await?;

    let length = res.len();

    println!(
        "At this moment table {:?} contains {} rows",
        table_name, length
    );

    Ok(())
}

// fn test_handler2(joined: UserWithPosts) {
//     let _user = joined.entity;
//     let _posts = joined.children;
// }
// fn test_handler3(_user_opt: Option<User>) {}
// fn test_handler4(_post_opt: Option<Observer<Post>>) {}
