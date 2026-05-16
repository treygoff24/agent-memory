use memory_substrate::{InitOptions, Roots, Substrate};

use crate::cli::ServeArgs;

pub fn serve_roots(args: &ServeArgs) -> Roots {
    Roots::new(args.repo.clone(), args.runtime.clone())
}

pub fn serve_init_options(args: &ServeArgs) -> InitOptions {
    InitOptions { force_unsafe_durability: args.force_unsafe_durability, device_id: None }
}

pub async fn open_substrate_for_serve(args: &ServeArgs) -> Result<Substrate, memory_substrate::OpenError> {
    let roots = serve_roots(args);
    if args.init {
        Substrate::init(roots, serve_init_options(args)).await
    } else {
        Substrate::open(roots).await
    }
}
