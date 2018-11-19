use beserial::{Deserialize, Serialize};
use crate::consensus::base::block::{Block, BlockHeader, BlockInterlink, BlockBody};
use crate::consensus::base::primitive::hash::Blake2bHash;
use crate::consensus::base::primitive::crypto::PublicKey;
use crate::consensus::base::primitive::Address;
use crate::network::NetworkTime;
use crate::network::address::peer_address_book::PeerAddressBook;
use crate::network::address::PeerId;
use crate::network::connection::connection_pool::ConnectionPool;
use crate::network::address::peer_address::PeerAddressType;
use crate::network::address::peer_address::PeerAddress;
use crate::network::address::net_address::NetAddress;
use crate::network::peer_scorer::PeerScorer;
use crate::network::connection::close_type::CloseType;
use crate::network::network_config::NetworkConfig;
use crate::utils::services::ServiceFlags;
use crate::utils::timers::Timers;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use parking_lot::RwLock;

pub struct Network {
    network_config: Arc<NetworkConfig>,
    network_time: NetworkTime,
    auto_connect: bool,
    backed_off: bool,
    addresses: Arc<RwLock<PeerAddressBook>>,
    connections: Arc<RwLock<ConnectionPool>>,
    scorer: Arc<RwLock<PeerScorer>>,
    timers: Timers<String>
}

impl Network {
    const PEER_COUNT_MAX: usize = 4000;
    const PEER_COUNT_RECYCLING_ACTIVE: usize = 1000;
    const RECYCLING_PERCENTAGE_MIN: f32 = 0.01;
    const RECYCLING_PERCENTAGE_MAX: f32 = 0.20;
    const CONNECTING_COUNT_MAX: usize = 2;
    const HOUSEKEEPING_INTERVAL: Duration = Duration::from_secs(5 * 60);
    const SCORE_INBOUND_EXCHANGE: f32 = 0.5;

    pub fn new(network_config: Arc<NetworkConfig>) -> Self {
        let addresses = Arc::new(RwLock::new(PeerAddressBook::new()));
        Network {
            network_config: Arc::clone(&network_config),
            network_time: NetworkTime {},
            auto_connect: false,
            backed_off: false,
            addresses: Arc::clone(&addresses),
            connections: ConnectionPool::new(Arc::clone(&addresses), Arc::clone(&network_config)),
            scorer: Arc::new(RwLock::new(PeerScorer::new(Arc::clone(&addresses)))),
            timers: Timers::new()
        }
    }

    pub fn connect(&mut self) {
        self.auto_connect = true;

        let connections = Arc::clone(&self.connections);
        let scorer = Arc::clone(&self.scorer);

        self.timers.set_interval("network-housekeeping".to_string(), move || {
            Network::housekeeping(Arc::clone(&connections), Arc::clone(&scorer));
        }, Network::HOUSEKEEPING_INTERVAL);

        // Start connecting to peers.
        self.check_peer_count();
    }

    pub fn disconnect(&mut self) {
        self.auto_connect = false;

        unimplemented!();
    }

    fn check_peer_count(&mut self) {
        if self.auto_connect && self.addresses.read().seeded() && !self.scorer.read().is_good_peer_set() && self.connections.read().connecting_count < Network::CONNECTING_COUNT_MAX {
            // Pick a peer address that we are not connected to yet.
            let peer_addr_opt = self.scorer.read().pick_address();

            // We can't connect if we don't know any more addresses or only want connections to good peers.
            let only_good_peers = self.scorer.read().needs_good_peers() && !self.scorer.read().needs_more_peers();
            let mut should_back_off = peer_addr_opt.is_none();
            if !should_back_off && only_good_peers {
                if let Some(peer_addr) = &peer_addr_opt {
                    should_back_off = !self.scorer.read().is_good_peer(Arc::clone(peer_addr));
                }
            }
            if should_back_off {
                // TODO
            }

            // Connect to this address.
            if let Some(peer_address) = peer_addr_opt {
                if !self.connections.write().connect_outbound(Arc::clone(&peer_address)) {
                    self.addresses.write().close(None, peer_address, CloseType::ConnectionFailed);
                }
            }
            return;
        }
    }

    fn update_time_offset(&self) {
        unimplemented!()
    }

    fn housekeeping(connections: Arc<RwLock<ConnectionPool>>, scorer: Arc<RwLock<PeerScorer>>) {
        // TODO

        // recycle
        let peer_count = connections.read().peer_count();
        if peer_count < Network::PEER_COUNT_RECYCLING_ACTIVE {
            // recycle 1% at PEER_COUNT_RECYCLING_ACTIVE, 20% at PEER_COUNT_MAX
            let percentage_to_recycle = (peer_count - Network::PEER_COUNT_RECYCLING_ACTIVE) as f32 * (Network::RECYCLING_PERCENTAGE_MAX - Network::RECYCLING_PERCENTAGE_MIN) / (Network::PEER_COUNT_MAX - Network::PEER_COUNT_RECYCLING_ACTIVE) as f32 + Network::RECYCLING_PERCENTAGE_MIN as f32;
            let connections_to_recycle = f32::ceil(peer_count as f32 * percentage_to_recycle) as u32;
            scorer.write().recycle_connections(connections_to_recycle, CloseType::PeerConnectionRecycled, "Peer connection recycled");
        }

        // set ability to exchange for new inbound connections
        connections.write().allow_inbound_exchange = match scorer.read().lowest_connection_score() {
            Some(lowest_connection_score) => lowest_connection_score < Network::SCORE_INBOUND_EXCHANGE,
            None => false
        };

        // Request fresh addresses.
        Network::refresh_addresses(connections, scorer);
    }

    fn refresh_addresses(connections: Arc<RwLock<ConnectionPool>>, scorer: Arc<RwLock<PeerScorer>>) {
        unimplemented!()
    }

    pub fn peer_count(&self) -> usize {
        return self.connections.read().peer_count();
    }
}

#[derive(Serialize, Deserialize, Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Debug, Hash)]
#[repr(u8)]
pub enum NetworkId {
    Test = 1,
    Dev = 2,
    Bounty = 3,
    Dummy = 4,
    Main = 42,
}

pub struct NetworkInfo {
    pub network_id: NetworkId,
    pub name: String,
    pub seed_peers: [PeerAddress; 20],
    pub genesis_block: Block,
    pub genesis_hash: Blake2bHash,
    pub genesis_accounts: String, // FIXME
}

fn create_seed_peer_addr(url: &str, port: u16, pubkey_hex: &str) -> PeerAddress {
    let mut public_key_bytes : [u8; PublicKey::SIZE] = [0; PublicKey::SIZE];
    public_key_bytes.clone_from_slice(&::hex::decode(pubkey_hex.to_string()).unwrap()[0..]);
    let public_key = PublicKey::from(&public_key_bytes);
    PeerAddress { ty: PeerAddressType::Wss(url.to_string(), port), services: ServiceFlags::FULL, timestamp: 0, net_address: NetAddress::Unspecified, public_key, distance: 0, signature: None, peer_id: PeerId::from(&public_key)}
}

lazy_static! {
    static ref NETWORK_MAP: HashMap<NetworkId, NetworkInfo> = {
        let mut m = HashMap::new();
        fn add(m: &mut HashMap<NetworkId, NetworkInfo>, info: NetworkInfo) { m.insert(info.network_id, info); }

        add(
            &mut m,
            NetworkInfo {
                network_id: NetworkId::Main,
                name: "main".into(),
                seed_peers: [
                    create_seed_peer_addr("seed-1.nimiq.com", 8443, "b70d0c3e6cdf95485cac0688b086597a5139bc4237173023c83411331ef90507"),
                    create_seed_peer_addr("seed-2.nimiq.com", 8443, "8580275aef426981a04ee5ea948ca3c95944ef1597ad78db9839f810d6c5b461"),
                    create_seed_peer_addr("seed-3.nimiq.com", 8443, "136bdec59f4d37f25ac8393bef193ff2e31c9c0a024b3edbf77fc1cb84e67a15"),
                    create_seed_peer_addr("seed-4.nimiq-network.com", 8443, "aacf606335cdd92d0dd06f27faa3b66d9bac0b247cd57ade413121196b72cd73"),
                    create_seed_peer_addr("seed-5.nimiq-network.com", 8443, "110a81a033c75976643d4b8f34419f4913b306a6fc9d530b8207ddbd5527eff6"),
                    create_seed_peer_addr("seed-6.nimiq-network.com", 8443, "26c1a4727cda6579639bdcbaecb1f6b97be3ac0e282b43bdd1a2df2858b3c23b"),
                    create_seed_peer_addr("seed-7.nimiq.network", 8443, "82fcebdb4e2a7212186d1976d7f685cc86cdf58beffe1723d5c3ea5be00c73e1"),
                    create_seed_peer_addr("seed-8.nimiq.network", 8443, "b7ac8cc1a820761df4e8a42f4e30c870e81065c4e29f994ebb5bdceb48904e7b"),
                    create_seed_peer_addr("seed-9.nimiq.network", 8443, "4429bf25c8d296c0f1786647d8f7d4bac40a37c67caf028818a65a9cc7865a48"),
                    create_seed_peer_addr("seed-10.nimiq.network", 8443, "e8e99fb8633d660d4f2d48edb6cc294681b57648b6ec6b28af8f85b2d5ec4e68"),
                    create_seed_peer_addr("seed-11.nimiq.network", 8443, "a76f0edabacfe701750036bad473ff92fa0e68ef655ab93135f0572af6e5baf8"),
                    create_seed_peer_addr("seed-12.nimiq.network", 8443, "dca57704191306ac1315e051b6dfef6c174fb2af011a52a3d922fbfaec2be41a"),
                    create_seed_peer_addr("seed-13.nimiq-network.com", 8443, "30993f92f148da125a6f8bc191b3e746fab39e109220daa0966bf6432e909f3f"),
                    create_seed_peer_addr("seed-14.nimiq-network.com", 8443, "6e7f904fabfadb194d6c74b16534bacb69892d80909cf959e47d3c8f5f330ad2"),
                    create_seed_peer_addr("seed-15.nimiq-network.com", 8443, "7cb662a686144c17ae4153fbf7ce359f7e9da39dc072eb11092531f9104fbdf6"),
                    create_seed_peer_addr("seed-16.nimiq.com", 8443, "0dfd11939947101197e3c3768a086e65ef1e893e71bfcf4bd5ed222957825212"),
                    create_seed_peer_addr("seed-17.nimiq.com", 8443, "c7120f4f88b70a38daa9783e30e89c1c55c3d80d0babed44b4e2ddd09052664a"),
                    create_seed_peer_addr("seed-18.nimiq.com", 8443, "c15a2d824a52837fa7165dc232592be35116661e7f28605187ab273dd7233711"),
                    create_seed_peer_addr("seed-19.nimiq.com", 8443, "98a24d4b05158314b36e0bd6ce3b42ac5ac061f4bb9664d783eb930caa9315b6"),
                    create_seed_peer_addr("seed-20.nimiq.com", 8443, "1fc33f93273d94dd2cf7470274c27ecb1261ec983e43bdbb281803c0a09e68d5")
                ],
                genesis_block: Block {
                    header: BlockHeader {
                        version: 1,
                        prev_hash: [0u8; 32].into(),
                        interlink_hash: [0u8; 32].into(),
                        body_hash: "7cda9a7fdf06655905ae5dbd9c535451471b078fa6f3df0e287e5b0fb47a573a".into(),
                        accounts_hash: "1fefd44f1fa97185fda21e957545c97dc7643fa7e4efdd86e0aa4244d1e0bc5c".into(),
                        n_bits: 0x1f010000.into(),
                        height: 1,
                        timestamp: 1523727000,
                        nonce: 137689,
                    },
                    interlink: BlockInterlink::new(vec![], &[0u8; 32].into()),
                    body: Some(BlockBody {
                        miner: [0u8; Address::SIZE].into(),
                        extra_data: "love ai amor mohabbat hubun cinta lyubov bhalabasa amour kauna pi'ara liebe eshq upendo prema amore katresnan sarang anpu prema yeu".as_bytes().to_vec(),
                        transactions: vec![],
                        pruned_accounts: vec![],
                    }),
                },
                genesis_hash: "264aaf8a4f9828a76c550635da078eb466306a189fcc03710bee9f649c869d12".into(),
                genesis_accounts: "\
                    05740fe832581bf6a0892412acfb9651b451c509831d0000000005dbf2a54718ce70a65cb6c7e08a\
                    bd6b24373b33574e16e70d00000000046458a440306108b9072bfcb59984fad0ecba5e6c1de46187\
                    01000000204faa14e0200020d0290be350f9dd1263c9d915cbe23bfb3b000000010001fa40000000\
                    1027d50a70000000204faa14e05a864ffbccc17d57674ee0db678cc347ec9281ea00000000002ea0\
                    3010861bc9dfd24ba6a9c25daa883b6443b04b566c030000000000083d871088b5db434af0385e24\
                    dcf12676a24191f4f062c000000000011f6a03f8a1c62efca1ccaa68b433be7518a42f5245e1d2db\
                    0000000004e3b29200a6dd0618146a400ae1b0f96a731dda2c3eb57203000000000319f94b68f8a0\
                    14c0411bba7e44707d8ce3ecd2f6c78f26d00000000007d1b50006119474e10409fe87014f0f3732\
                    9942f264d41e43000000000005f5e1001330c9b007dbb2eaae43b07882c23478444b0bf100000000\
                    003abe203221fe73de48a5728ebca0df7117d65ccebbde5f110000000000753afc8028d78f338d97\
                    ffb63d471d74403a3bc2651a100a00000000016b7f8cb5293358cc662c49cad9d452b268d415eee8\
                    f935b50000000001a3f2c77f351a1422e9079b4014366db376967f1d38e0feda0000000014f46b04\
                    003a0111c0a3bfcba85abb7fc15c70cc76d9c613f80000000000cd3802f258a510bb93bb432f3f20\
                    f775ba5d24cd6c2201c300000000012d935e80590ae4d457fd6a2d6ff1a02ada5f626ff5750ae000\
                    0000000447d1658f617df9944cd4a58c214305ad1f84fd83288005ae00000000048e9afbe077db9c\
                    a8efcaef7c4ee73df7434e0c9f55aba4c1000000001c025ebfce8291cb3d6dcd6a57e41c9b5a57d3\
                    e124f4d3e10c000000000c393e6d00863e303e23ed5c7aa5c2a65ab7e62c057f781f870000000008\
                    26299e00b52a4999e713cdc70258851e5276b69c01dd86f40000000003d8d5eb806533bb559e5327\
                    e2b49356e06b19dfeb7372c45901000000024d8caec0bc40dcd372a89c9283d3afb8afb3403c5c08\
                    b59e000000010001fa400000000126c65760000000024d8caec0da88f36168bb3c6b0d67642b9f06\
                    45d18599e20c0000000002447d58c0db199de0d21cf3f990c1a9b1ceafb315e8c73c3a0000000002\
                    c41dd8e8e161a406e95ca4c9b2781c56bc8848c3f6a61d9300000000006a64f040ffc1fa35dbe497\
                    eb815d574df3553c8a0661325b0000000001530e8db00e7c86436e4bdcdc0f539ba6c1bdbc5f21bd\
                    8a3f0000000000684ee1801bce364605cb171636b5ae4accda5d058d267a590000000040091a6458\
                    203a483d03ed864ec53fc85035229c864f936cdb00000000031c3af6b527d71fcaa22ae63b06f555\
                    a37240bcf4a96edaa100000000006a0b94a42cb6cc75b288ba98f5c9362a7ab5fa49a92246380000\
                    00000055a310b04684d5f2c67e7391f3de1f4d858cd72d1d430e890000000001ff1c1dc046c690af\
                    6ffd722e3988109e2b8250c52c53b1da0000000002098a67806bdba8f298f9b37773b8c8a4116c3f\
                    f33f41f92300000000001a13b86073465fdfbe5852c5c2d1dfad9dc2345fa465a945000000000120\
                    8c438081344309386f7408529103239f0c831068cca53d0000000005a4ba811d916244ae40498dcf\
                    7d93842865345aa8902e5e7f00000000041314cf0096157bdc37238df523356568db3f21a23e64ff\
                    5800000000018c5ef280ad549ea164476e2ae30eb80bf250c9f1fc68fc3e000000000165a0bc00ea\
                    98d97a921f95de5713e935dacef968ae52e194000000000ba43b7400f688830512d6827ed36ebeeb\
                    a186d275dd86c4bc00000000014dc93800122664ac12cc191e67afc9dad13a55a3c3019928000000\
                    00008725c14f25a35154812e05ecffcf43137f97be92f134b6bd0000000001d562f6c03c012e4b4a\
                    fcb49a29e4ae4a6c29135f94b864f500000000013a2d13c04c89eb5fca10b0729151e0f578f51b5c\
                    b9cc78d20000000045bd673100603f126807abb650b4a061fc39970e599f176e0d0000000003a55d\
                    bf9c6f1f235ef03f7f1cf32db67a315ffa2347a3910900000000004c5a82401cd00cc47737684602\
                    ad7d3b653391e7b1581bcc01000001977420dc006fb0f9413145fbadf20146b6194898a62042ae5e\
                    000000010003f48000000043e8b024ab000001977420dc0074c9f67ea50cab0dc9cd85590b06b98d\
                    5abf5778000000000067a6a7188270736c17075cd5784c8ceaab260876017277c000000000003acc\
                    4dbe94e4d8a18f7a8bf2b2a53e841ec1b6624d3fd7f400000000002e618c8296e93db661d6e4d5b3\
                    575b61f2be22ad2b53a85d0000000001a13b8600c7ec87bbf1ac6d70fd5699adb77fee4883190cd3\
                    00000000010f337d80e91515a9ed2a740d9913af7d13835d29795ec3f7000000000147b05432f2f6\
                    4a832d172857493f6d7ef22c0913fa8ff0bd000000000502fd6f40077f86f89d9afe4fa5add3234b\
                    3c4e3b58435a930000000002098a67801522c8d201edf3a49107c0a55ba106a59bf839a300000000\
                    0001312d001743b24dcedef3590939ae58aef13bba779f756500000000008adf9c4d49c10cf90677\
                    48ceb1fa0f8119596e44ad4780e00000000000ffbc853d513b17d490262fb91f78b1f48aa16556b3\
                    df9cde000000000071d849805d5c11f6e71c2f258036000db4fa4264407e558900000000018a4f23\
                    a263975584ca764c5aaaa407e1349380d0b6509d900000000001a13b860074b88e8d49555c9c6069\
                    7c0975349de7bcd82bbb0000000000684ee1807ab7853fa977a4f2c65ed13583f6b16fea4fc86d00\
                    00000000009896807dc73b7bb818a9c74183ee12b4d3a0c4a4aebaf5000000000c409cc7598126f5\
                    86279aa5bed7af570a99ad6c43b745f19c000000000046f25398856a672bdd1a333811f377f6fc88\
                    1d465b7bbce60000000001802cc29f87d808a720b0cfcb1076aa345908fd1ec169b5230000000004\
                    1314cf00ddda74a5140a42cb37a1169dd348b4077397eb3d0100000011601c20208dbb4c872de726\
                    79639392d5482e26efe6f4485e000000010001fa4000000008b00e101000000011601c2020900aca\
                    2847105782d8882fb3f43ea05ade9c64ef000000000015222209a0cf0841bb303ae4754072112a95\
                    9d72cd7b400a0000000000ee4fb0c0d1d1b34d5b2f2b336268109643113c65364269290000000000\
                    7c7a7480ec15f7168ab643b6736203858bf340cb690a189f00000000012aba8c32eda0d538062601\
                    adb8f73f9174087132e51f877d0000000001596604ea21e8359101377f4108ae1c82b17aa28f41c0\
                    44b80000000000342770c0508e08330ed1e7dfbe753d311195bbc70008e546000000000271a9fba0\
                    573f6fbe9936a9917c6fae7265723f8222e27c940000000000315b4334637164ca1a9de33fed5b71\
                    4743946add2dd9262d000000000137bb77807efdd0064f4a3bf50df9a67713227e20b38823e30000\
                    000005312637b48fd1613d58f353f00f01e07acd13819087f0cc950000000006c366d50992887a64\
                    19696791e8749784fe05568c5e40e1cc000000000061f325ba9b2045abcdf7f83abd506d2033841f\
                    283cfdd8460000000001a13b8600bd215bf873d59bd34515f13589acd231e88ac19400000000061c\
                    9f3680c1a1e88e2cbe3f8555539d26ca28f732d242aefa0000000003fe383b80c74ce8379dee3080\
                    8dfec07c8be5d30f388bd97100000000009fdcc4d9c7d610319e587b31d7e8c6580f84162e4ae5dd\
                    290000000000c62f7940f704df51a09505aa14ab6840bc56c6f2c011f7b2000000000067a6a718fa\
                    f22741f8b74ed648058b2e9b309ecb7aaa5e830000000000211d7b910ccaeda01a8e383889bb4161\
                    3e5262cb89430cbb0000000000d09dc3001059c5ece6b2e24d34c663fa9ac3c274be5ea6e4000000\
                    000f4fd8398011651fb231f2bd049e99fd00c37b4e0efa2e2e6e00000000018ed329ea12e425986d\
                    5fca768ee9928782a9abe72c8bb73b0000000006aea73f0016dd7a4b71cf8bf1b25c872d1e827459\
                    e910adc300000000006949128d17b858528b3b2f620906b989e393bdb5cfc3ed250000000000ef70\
                    625027261965b9b740c1d7bade614748e25c30aa88b60000000001a13b8600c81c8fc0c210c6aced\
                    eab6a6d74d44b218c2b6d101000000131c84218052b787460800d22744dcab004cf6fa3e6ce1eea1\
                    000000010001fa40000000098e4210c0000000131c8421805dc7986ef6c46219dd739a6b7af13f5b\
                    19e3bee10000000003aac5ed806a1f98f5d61271747e8e7fc849c94f865ef9825e000000000b90bc\
                    5a3d82ea349d70b97645a0261da529f27ad1243c072d000000000342770c0090065d6bbc3ce0d0f4\
                    510225e2a5d1b0445d4fd3000000000039bafbd6a014c8b966ea695ed1d2f5e47ff50c95ba871953\
                    0000000001a0dcb9d4a17c4dd00c092a8a2004c5f7518c9acb4643b14900000000001941432eab01\
                    005cf2e76f6ffe8e55b4f70dd426463b8c9d0000000000127a3980d7752a98401c5c189ae0714c9d\
                    08fd26ea6be2b300000000003d57dd94d975f4df44d4a856deb0384e39591953030c36e200000000\
                    1f55523dc4ed2de0dd99eb33f84b9d1c8d77924d0e1ee07d480000000000c84ba280f6ab6417dd46\
                    d9470ccaf1a54e811065049dfe8c00000000003fc6e780fc0ab2982448c4d8e322e63d5e3a773d4e\
                    110b0d000000000c393e6d00fc3c5025807fb83e82572b31ec219691545c906f000000000092abd5\
                    ad1fef9247c66bd40c179322d5ad14bee131d2ecdc0000000000f601593620d7f8dc95bd8401d14d\
                    59907cc01c1cdeb79ff300000000001494dc202dc0962730686692e0a86f5c339d298307ce908800\
                    0000000826299e004197c2f41e49f8c1f14daee9dc2705ea251005f8000000000150e29017634bdd\
                    de908fe8e811ad363f4f243533f9f46d9400000000025c24d91ca3a57f5e415ee1233f6eece184bf\
                    7ae720d7c5e400000000007a484b6aab79ef99f1fc70a36055e2117e2c087d8f49c4f10000000000\
                    0bebc200c3635b4e2447e294aa387803ff85bd0a9756da030000000000d30ca447e3eab835cb3b8f\
                    7baf13a7a24c83c639a881c88700000000067f19c8c0e6c088e97dce68d1830259283111e010c243\
                    f11f000000000bad2c4580fb463f8e84f7abcc66d0e73082beae440f48a736000000000238414d97\
                    060c290288450d7d195688cfc1d5b9e01b1052670000000000f0be369914059a855f43070321053f\
                    ef753df4d6d9e0deca0000000000684ee18025900207bfaa657ec95a88ab7374d1cdbf3a1e5e0000\
                    0000462074568b30d148ddd97cee63f0f7b287fba57bada33c481a000000000011e1a30033678b8d\
                    e1b5899b816d06775798044befeaf089000000000187b42f203b879a4a9566c44a5c450a7b03fcff\
                    38f21a2e9200000000059649f4cc49dc2b60c3ab52c2730dc9462ee9a0fd9e446efe00000000030b\
                    eb14f54ad9a924d25b4f8b872bacc145c88caeb25133e800000000008f0d18004d4d450f5fd6ec65\
                    16b5b2297b42dd1a22a9f20b000000000163d6f8806a254fbfb5c5a1263f8f80a68688953ba6f28c\
                    630000000000483e5c766a325f9ec8a2d7e506896019a53f918715b15ee8000000000a7a35820075\
                    cbab0e9e6e4633c7dc3f5da9735de2fb670f310000000001caeb96c7880ea341bd788c56cb7b4158\
                    6cd7c64b07e5eb490000000000b57f0340a8819828241b4013e6673080de8d85ed3f426b46000000\
                    000136906500abacf682e6047c7d6322a9ec6e14e774e2b4c1900000000000ab10b980ad89fce19b\
                    294eb1098beca9fbef757f59c3fd870000000001646f8f00e094f561faf744eff949370ec8469696\
                    8dc6a9d6000000000001312d00d0f0e976f65db8c3a929be04975144e071b7c83a0100000a680eff\
                    7820ee097e8e28cd348632655204480986ab2db95db2000000010001fa4000000534077fbc100000\
                    0a680eff78203317554a8eaa82ea4e1038601c39aa2b2ba24de600000000005e0a29ce3d87785fc4\
                    40fd50ea63f5bbf92099f0608f9878000000000200294dfd40ef10fadc1ea3e58095e22908a23236\
                    67c6fffe000000000133deb280d8ea82ac1f8c63395069511e4ede1b264868f5e40100000056e089\
                    936042eb86b8e60837407119c32d0c3cbff56c31590d000000010001fa400000002b7044c9b00000\
                    0056e08993606a3bffe260edfed0e8ce942dd50866a6e0cd0ba3000000000125c90ba06d1a5452ac\
                    a4820a817f1a491e661c290ceaca4c000000000045de5a4092d619680ba162e224f61a4172f8c15f\
                    4a7ad57d00000000061b0eab709ec9f08e2eb1a882e32f5622ae2ad0b4093db23f0000000001babf\
                    f6b0a0bc9f4fc0650dcca786021b045be2a3661edbe0000000000c1467fa73a6f46adf12649bc9f1\
                    9b63d0cd87746428e2750f0000000000023056e8c1d7463a55837d93ce192a835d5eb1db7ab784cc\
                    0000000002540be400c2248af122b76deb57c5832344fe4ea79080e1c50000000009acdbe86bd30a\
                    a388dbba79b79b7f67d0187adb47be653880000000000560457080d65011ca76553ccab08fed3d1d\
                    311d69213e7d22000000006beb1dc508ea541645449952338257133c6faf62fdd16b8ca100000000\
                    0110b44ab60c4de994d1d5db1fcbcb6cede802023b24c6f25f000000000395e95a002059262cc1a5\
                    fba3e4f4be85ebf7b8bcaa2d880100000000005f5e100021615c1a1f27bc3d194f35412b961544b7\
                    0101c900000000019f24823d6b2445536fb5df48c86016448f7a6998b2cf6e2200000000012e7e5a\
                    c06f4e4cd6075f0614b38048b423c2b988a0f051de000000000138eca480998dc7e21bbc40d4719d\
                    9411d6d9ef4777fcff1300000000027c975f58da5315a50def11b5ae54cef46d9c124c571b3a5500\
                    000000003d278480e07bb817040fe712689af2c9f1958df0966462c200000000037e11d600eb0d5a\
                    e8339739aaf6cf764f0ca1fb9b4473f4110000000000c5c7b724fab858a2e3b568e7b9ceabf8c401\
                    a501cd2924a000000000000bfe3e1c1f9a762b8717eec2a5a76f6d8764c8b3bb6f0f340000000000\
                    0fa56ea0328e0b834be388c7070ec8ea106f844413c4d75d00000000187aa4d766cbf350f2ee1d06\
                    ed710ab527d870e6081f075a2c010000000409f4b0205d0286d9c3cbc8cecb90f09feba76cd4bb93\
                    cb8a000000010001fa400000000204fa58100000000409f4b020806f1c465dccc9b672edd4e6d705\
                    2d11bfeeae64000000000057bec2e68c7ed38dfbaab8366a092e339bec61977aa0dc350000000000\
                    6674350ea767a9c973ab1ba914fac41a2e434f33deab5e1f0000000037f17dba30af96d946f3eb83\
                    6866c95f28ea3521e26fd3fb8500000000006ce92f37c3135c86645ac965c93d3d1e1a9f0c1ec586\
                    a150000000000794219580de71cfc3487a21890857499d6ce66bfac420e645000000000ba7366480\
                    efb2e6dfa864b8f336a3d4609672385535d8c26000000000013e49ef0000878b8ccc0fa3eee7fe2a\
                    38f5c073912f432e5200000000028b0b363d483edf51a60f8ece9e358c143150f3cce5e162af0100\
                    0000114f91cfc00578229584b791b31bb070f47303a8332e12a584000000010001fa4000000008a7\
                    c8e7e0000000114f91cfc02e9727f7d4d70fdd45a6a046082f07ec4165c4ac00000000006f319bc8\
                    331e79690bf28c46e39eb5a5ba093e9a79ae47de000000000160fdbb034a96572eb401098379e114\
                    6c7483e1f8035f58d5000000001080880cdb4c2c75a3afb9674337f0faf5293df6b960822e7d0000\
                    0000022300f5804df5287455f5c039a1dc0ed915f6b8a824b1a7160000000000a7f12a004ed51a7b\
                    934f15910c2299d4b9aa260b3484710a0000000000d0052c80512bab2b37bbfb95f8b33b98ef47c7\
                    8e66064d630000000006d1334a326331111ff15dbf53b7b9f5cd73fd0c8f045ea021000000000000\
                    989680731f08d1d137c7f273c0f8889a9f2263e20ade5d000000001c087f4f48a0bc8224acdea4de\
                    97a8f172460298606e643ab40000000008bc023680af6760ddac22c7ece780f186d4fca65d6d0e8b\
                    de000000000017d78400cac546fbaad0bfe34451aefb4eed69c1939c5701000000000137b6946ce5\
                    0945886192af8dea8720632798e4e505f9ecb800000000002d346134f730dedea7d927a1e5fbe7eb\
                    7e3c56bd3ad3e04100000000013396a454fed34126d7c0ab4970100a5ba12ab777998c39b9000000\
                    0001a423d36a0183f0aeaaf1418c59d03f80efee627c1b63fe2400000000003348a9800579d97312\
                    e23d2c406fdd63b5664645c2be24b10000000000255a69470eb57ffd9c9da44ea41e383a3072d2aa\
                    4c6b2f1100000001d33b9ef3801a0278f6a02f67cc34b4d141bd10dcb83e03606600000000006685\
                    1e0030a095cf88135f25d8cc94bade280f8b1927bf730000000000684ee18037bc4a50b135b5409e\
                    e203c53a4993da086543960000000018053f4e503e413480be11647add7fccf86dde36431f8ae00e\
                    000000000479ef2a0b40d061215b0c3a409d059dce0248cc265e44983000000000004229b5df6183\
                    d929c38ff445149af60c2d769dbd75fe3cdf000000001063edb70062f891ec3bd0edb5a3b691b489\
                    936681833494ef0000000004a817c80084dfd47308fbcbde436ea9e5ddf0dbc272fff07a00000000\
                    005d436d60acfd06e1be0a98cf35df43278c2764d2c8dac06a000000000002faf080d2af75102b57\
                    973c3d0487085dcfca0700b443b80000000000d09dc30033dd5720e8d2af3a608d08e53ab7a3bee7\
                    e7119c0000000000ac79beca35df441284651da796c5c66015020f6b3d991fd400000000005c44e2\
                    f82a2d8ed8eb934bd974f07823dfb76c040294ab890100000011601c20203b111ac481d99d66c14c\
                    82499d41555364a8792b000000010001fa4000000008b00e101000000011601c20206120581e8a53\
                    dfe4d1547671d21c2ca5e5fdc7370000000007558bdb007267dbb3a22829727f0997f0d664965184\
                    7c5377000000002e96c8d3c1835514b8b1b59b96077b3707bb4cc058109179180000000007fa183d\
                    81b98b8ceceafa4b6f91093b0d777f2e0015ffff34000000000002e40d20cc595ff867911ef846aa\
                    7f02009c1ac4d053c8a90000000000684ee180d3b10838361a89c26d752c5c9b5ebc9a1d5d2fba00\
                    00000002098a6780d46daf32595154e05042b82a0df1984b054e3421000000000035327820d9917d\
                    0857a3c4780c019ff59e1d43fe94db88f2000000007832e5da800da58d29d47509b40fa5475ec0dd\
                    b3449019e75300000000006669a05626f284893563564ccdc0de3eaae6b8f111b67c4f0000000000\
                    ce1ff1c0374041f94052ea0b25e35a68e756558a05293678000000000ba43b740046906208eba3a0\
                    d49d071ef1ee3b254edea5f367000000000061a121467500f4950f1523f3831f7ea041c3798d454c\
                    cce4000000001911c44da0b0a17cb0826e5324c150b73318e3a41e6e216f680000000000240a4835\
                    bb5e954f03189d366462374a9f1b7e0a7991cfe1000000000906880545e25921c92ce6eb5cb72989\
                    3eb63b6383bab0959a0000000000938580c0fdcd26b48b560cdcd71c84c2ff636865ccecdeea0000\
                    00000241ef913bffa3195e28fa2794ab1aa4fec66607a75ce34d7b00000000000098968015902ea7\
                    ec41a980b54a156476f27caee9a33d240000000000331c69601edd3d7fe2ddbb393b74df370b01bc\
                    feb0a527bd0000000000409ac5c8243ead400b67c54a36d61e0b62282098907979e2000000000000\
                    9896803cb0e8990f82611929bc474497ca5bacf5c330e800000000052ac86f4c804e289205a99573\
                    af5efdcbfc678cd20f9dad0b00000000000fa56ea097e25f90670f2ca2098b1dcc0b415a0c33c56a\
                    68000000000023c34600d86abfe87de20af5abe0959153cf9c4ea3fa3e02000000000067aea9e0d9\
                    f3715c07675d1b032496c889c55f6dbdf82e5c0000000013d8ccd799dca237a838af7b967f5ba979\
                    7639fce518195bd70000000000787a1e83f3af000d6330fd209b407138b04458b97dd717a3000000\
                    0001e495f480024fce32306c34bc7f8e1a92d10b9c2a52460b290000000000c8081d5808133670db\
                    b403f2a07de2c39c63f9539d49f1fd000000000037cecde020000064eb22f7560ce97243d7026e88\
                    8eb3b82c00000000012a05f2001f70b02594d0f185a5ed199aba483ecda67057180100000008d419\
                    d0c09d514b9f50e55ebb4754257a46a2a2e517a24b4f000000010001fa40000000046a0ce8600000\
                    0008d419d0c0a5ef4173b7a61c46decf34c2eb29ceb18cbbe883000000000072bd2b40a68a0ad2b1\
                    20eead72d2951bc3deed5737d692e00000000000ddbab200bcf8276dd67f1ad3e042d547c36ee6a0\
                    f4ced0430000000000152caf50d3db24e4edef0292b1a8c83059578bedadcd93f400000000041314\
                    cf00d704e71d1608594547062006dec5027d57fb270000000000042108997ee471b22c259b35e15a\
                    8fc062239bdd9a14452c480000000000ec5b17b4e7f3d74077fcfd9e064414bafdae3a97a3708a91\
                    00000000014c4bbfc00206200f31d124c7c13ed193c128c9f75fe920a8000000000138eca4800c1f\
                    43096f66524caf3c35dd23d1e6c06e6e106b0000000002e7323f9412440a4c59d0e43571d4093082\
                    c49f2b2016708d0000000000684ee180198b7f5e9328fbe4faebb1a60e5795f456834e4200000000\
                    010a3ed5631c14ba1b70cf161527e70792f55f1f7137b66b1f0000000040c8fe0e80407d2944194f\
                    854556b8be8a4c170c31e905248400000000009c7652404fe4b5dcc781dc0dfcbc87d4854ab7bb9d\
                    ed6aa5000000003b93ad4f8b53085ae0b8a590a2c1c1a2acd9f52c91a1367d2f000000000140f703\
                    7964a138cddb694e0c0d9f8a044827df5bb0407e300000000003207d898065e312a47ed681546beb\
                    671acbd9b303fd4fac86000000000071cb66ab66dfada22175252ad3676ce639d1de8fa4c998d400\
                    00000000684ee1802c8d0cc9b89fac868125b753edd0e844027d582c010000000813eae6e072a632\
                    4656c525a30566d538db0d5837a8b9162f000000010001fa400000000409f573700000000813eae6\
                    e0929dc82035d237ec91b1d4c22bcad2fc4d98d1a900000000023567a776aa507876f9b01dd7c16f\
                    7dbd14c7f7a7142878ce0000000000395e95a00afc50e939a490dd52fc82c2d82eec38c50dcce001\
                    0000008bb2c97000dec25d024207468384659c8e9353e72cefc2d18a000000010001fa4000000045\
                    d964b8000000008bb2c97000eeaabab66c4d980a47b08417ce77864af62830a30000000002098a67\
                    80f43404ff8e8acee74ae76b2d06f05bd047425c5d00000000005c631f80f5f6729f5526669000e1\
                    767bcce1d91f2c215f300000000002c523eac008aa05fb751344b6efe0cf247dbb1f431d3dac1c00\
                    000000003e95ba801e2e2f4c41196324df3e3724503288c3d84a2e7c00000000746a5288002ae019\
                    a9de4a31bb5852707d5571b5e88183c549000000000c3c1046582e9b9277c52f5e646c1965b69714\
                    ee394386f54400000000009babc17442365c257d876ed295f47613dc9cdb95012c51dd0000000000\
                    3b46dda079b461441586fb9812d404ecc15b4e78b79abe1f0000000003d152a24b7f3396d8f26fff\
                    6cf911cb8287e3496b778befa3000000000094ca3bbab0e4b70fe24655f96eeb08450a078ad9f1be\
                    58ed0000000003b23e02ffb66d7b506de4584c3ae167912aee3d52c5b473fc000000000049f51b00\
                    c15c07e7f552c7e8e173b26583353f1180f365f6000000000132aa7840d12ac6a1637219cb61caf4\
                    adbaace5ac09c3a4790000000000684ee180d13d637e44b6a6a168d4c81483decba243ceaf1c0000\
                    000004a817c800d19e5f6c829ee914449c716e40c685c9b1861ae800000000000f8abab0e79c5b9f\
                    582406f04be282e899e204d44fcce0c60000000000684ee180ff22ee28a051b9d4d82a071a3ab5c1\
                    f932cd9332000000002795c6cd4105de50edd590386acfd44d5020e7edde99e74d860000000001a5\
                    60a3cd06201c908d79fa8362da5d04b7e15f1a5a087294000000000138eca4800adfd27c955d2d90\
                    534dff62afbb4350c3f44eda00000000000f49552421f3c2c08d867f8fd33f6b62cd63b6dd9ac5cf\
                    ee000000000092dda800578da5b3a84b86f7eecd0653e5f63607c019b5fa0000000002550315407f\
                    e80075bc2dfccad7f54ed8370dcf84a1bf1418000000000492d96eb49b55f5b8a72830707a02245b\
                    6053da607bd90b8c0000000000009896809e039ad01be1ae635c72622656f7b8bcd82099e5000000\
                    0065dd083700a953cc697390c432fce69eaa61e33371b5a959e600000001e978223a49ac6384a7db\
                    76ee9c64646f608b79ac6632246d7400000000012e7e5ac0b72933f1219edc756ceaf14214ab53ee\
                    e9e8437e0000000002c777c57bc2bfb727dbaab1d0fc9cfb2e8746ac75b8407f7c00000000098cfb\
                    8700c54aead3b5ac1ee205da256b41804437f780240e00000000104c533c00d4f54b013a99dc08cc\
                    e6d2ad642d35787a5e42b90000000000d2aa7256f25181e3cf31bd219b876791fc32fd1820e5cad8\
                    00000000039c2e9380f4b7ae4c803dc4c07bbc6c7e92d5178bf962c0d800000000001a13b8608fb4\
                    0d2c76196f7056d025b5fff22c088684a94a0100000011601c2020f56ef02848b5acf1f9435bc922\
                    4e897d1287d49b000000010001fa4000000008b00e101000000011601c20200888205e9b7c7896dc\
                    debc38bc7e416575f1f2bc00000000009c7652400f80a4c14e53cdaba4cd79c36a09017db7afc8ba\
                    0000000000026cf897108318c309eb6dcbf4fa85346773d6434125ada9000000000109210e0415c9\
                    3b19eac52bfa04d880d4cb82236669db52ef0000000000a475836ec23e5784493f7e4ad25219e70c\
                    6e4487acbd870101000000b67121fa20385db37a526758a272367d61da486e1654b9d96e00000001\
                    0001fa400000005b3890fd10000000b67121fa203d4126a68d86ff53e5908d021a22c53f98c21fd5\
                    000000000012c684c048ca3981e7c52f2b9353fe1163835cdafe5f2f120000000001a5de84664b70\
                    ee64b74ce6cf713eaf8e116551670be3e6fa000000000065f2a2007bce146f173dd3a1e84276d625\
                    b761771776ee5b0000000000303c0a38a04e55a4003799e435d47bfeaa3b3e6a6a57a34a00000000\
                    17129c9720a313074bc7a5aa43571b21457bd6d0129d7836f70000000080466eb366a56345b03cb4\
                    c860c192ba3de1fa8e564ec851e000000000007cc344b1bc06b9b063ab4c4cbd4171892a7669a705\
                    b2439c000000000000989680bd4f6ff16b646c12fc8416aedafd4b28a50de65800000000006423c2\
                    1fd1616892ad69598fccfa137b2e9847bd56339a6e00000000000e5f510d17bf663e7be6b8c2c7d1\
                    1f58b7426b191eca51380000000001a13b8600196d0c9509fb3eb33cf53ee586c980dfb93929f500\
                    000000041314cf00254a86aeb1ee830e2abe881465e9b44aaa0616f900000000035cbf44ce8cd4c5\
                    85181ab611f9438c4add7c1d762bca0cbc0100000022ecb25c00704110f7d20a90dd98956ecda84e\
                    4d06c5052f52000000010001fa400000001176592e0000000022ecb25c009bc99e7fb114995d61dd\
                    9702d65a0a0604ef4cc0000000000005b8d800a71736c46bdc35b7f34ca110b7a0c5bc7462db5900\
                    0000000342770c00a89a1f99ab8632b5f3c4929b4cd6e134e5ed1227000000000489a31acacff0c1\
                    57fd95f40e4f55fc3379164ba89fb7035800000000044cbd9643d2313b856afee465ea334d4d93a0\
                    17d42bbcdd39000000000300017f40eb57b5ddb4f6b4b26dc904eb4774fb54c103c5040000000000\
                    01312d00f6b1e28e72ff3c2a1835de63a3f39d0fd0f0239d00000000001dcd6500ffe7a40e607513\
                    cf318aeffd3929d3584effb76e0000000012a05f20000d059cbe94d95bba01a240d52d546b343733\
                    22750000000000684ee1802380f00a6dea9c4ed3025b2285e5782946da654c000000000039643b58\
                    2f0532e3fa811efa3b932a2ddaedbd70e93e09f700000000004d0f8b8135d6db768f973293286e6f\
                    b5fbe33eb39ad3acee0000000000f88ebdd9438688bfbf72e0536235387049be3d61699963a10000\
                    0000060ee3f3e9450caf899165000b760875a732931e87219b213b00000000000a6e49c04e4b7ed9\
                    436ab4e87f74bc99c5fd286c093900030000000001dc4ec15c6d8726e3368647bab6c277fcabe54c\
                    e6372247f9000000000019744d95a2b6b0994a6384acef8c3806679c07e12b37a8ba000000000022\
                    e2f820b966aeabb1aa99ff56161aa3b2e81c55f08087310000000000d09dc300d3153651f0aa24bf\
                    3fc93f366ce0f51aeaa0a4e6000000000138eca480fe491d75413226b18841989bd4b54fed1b9532\
                    b7000000004c2b7a059d0266e4eefdf592d3c703d6fe62639f783b59cc77000000000038538e407b\
                    66d98d269f9a167a40d597b3f60dafaa6211e0000000000000da33607dcb9c7ac3f4f91aac4fca50\
                    297cc7bbcf3bb1780000000000684ee18082c11bc006f83e417069a8a82f46b8a5e5c5cd7d000000\
                    004847a1208e890a14203886c9a2017c8c8f5383948c5df1fe8c000000000622690834c867ae9238\
                    f36c61a8928b28a2d4cf85ea1febdd0000000001a13b8600cde48045e827e444e37c6e94c77ab99a\
                    09c79f0700000000018f7504adcffd98c68dbb67ddaf1df3e93cd2348e265925f9000000000068d4\
                    6530ece15f774b59db1067a7630d83bd6d2b6609139600000000009c76524002f28d4aa38fb43c48\
                    9f6079f9661b2c82814a41000000000826550342fc0a421b7e7461868b22ea3ca97904e3e00d02ac\
                    0100002632e314a00007a0a5559ef7c4919641aac3fae26ede383cee84000000010003f480000001\
                    e8f1c1080000002632e314a000159401f18a8729fb1cb8f9b109d3a1f97bd7a4990000000000350c\
                    528054dfad16a2ceb22a494f753633ca3ec9aee683aa00000000001a13b8605db5c1b9eb21f1af8f\
                    911bf2fd80965358d41bbd000000000001677f40840db01c3b1561eb83a88cdf544a4fa9e238330e\
                    000000000138eca4809a7427f9dd3714af7146795523a522f2f0682b4a000000001b5890c680afcd\
                    31375b9c4cff3c6a8aef3172eba8d1ad0a0c0000000001624d0be3b5343ff3950a9b2cdf71ae3209\
                    66fe6010d1bde00000000002a0a39ab3c4b9a0eca608ac94e1c681fe33b1063559873e5000000000\
                    0092080880c93f54a50dc362e9e1e65735bf200865c4a24bef0000000000764e8bf0fe5daaf033e7\
                    439c816abcb09a7ad5387ed36975000000000000989680013f95a9c8de2d980a7c27273ecf83c362\
                    8bb2bb0000000002c6c8640a286fb401991353df9184a42d3123cca9e1082b9b0000000000964480\
                    147b52da3e6a002af710a52948a2f8090a210e4c1a0000000028bd9ee9008beb924711175ff4e812\
                    a6fc21e08fec6f2284e100000000000fa56ea0940620e9dac29a26a0b843cbaf2ade43ab06e17500\
                    00000001a13b86009d4b212a654d3a7f5291ae8e01e047ede642357b000000002213dcc159bb9445\
                    bbe97b4c60987aad3bea347bc1cc663d4b0000000000165a0bc0bee8fa6877d6638c691ba16a3832\
                    1553264149ae0000000006ba930100c3905a80c859b51efac6439c6048e94f3020513f0000000000\
                    b2d05e00ce75245780eaec0b3d6ea1c145fbd4fcf708e7db0000000009c66dd6ca7c4c7c0627b571\
                    189a6e1ba050458b602b23e8310100000056e0899360d0cc527617d8ff3f08ae73f81156ad2e3d19\
                    2b46000000010001fa400000002b7044c9b000000056e0899360d31897834f2ccb5af3bd94e39e52\
                    bb6523b45cec000000000277502b70da370d9816d9a5d358e6aa77df985cfab7cccd580000000001\
                    d241dcb8e2af5553dc914bdfef533e7b1890c4187ffbb28800000000a74a5990907d98a26ee76726\
                    f93445c657d73025024d8cb7b601000000d862a64600e825b2ce65d619840c128ff93977f15fedc3\
                    acab000000010001fa400000006c31532300000000d862a646002dc3149d643d730b0ecce459eb87\
                    8fc82759d0ee00000000016b2464f0713000d910ad605e82d0d8c263b98f75f41073c10000000007\
                    68aa408e75196974b63dd274ab64476a85e465edd283a3290000000000441b34cea3c29055e40ae3\
                    4cbef5231155438bb7dd2acf460000000003bea12b79d658a417d3a4d5f6114d97232af397806845\
                    ddd5000000000005f6a93fd991f9cb0839880d8765ae9c41f201563247629a000000000b718c1b70\
                    de50fce33fd70b6190a9665d55c816b2b8c48ad400000000135aef2280def1156d13ca07d7a425fe\
                    689209b01268b6dfa60000000000edc6cadae454e14be0615b9b2ab363ee1dbf4ad900962cc50000\
                    0000006632d82f083dd65ca09a6f69a59edc2bc98d39ea6d621b760000000000b0fd72c00faac947\
                    7e2212aef6d7aae60acbe86b65bb48c600000000029a61430011500cdd6852a3383fb0f662e4847a\
                    451394da3e0000000001c0fa2d2b14ecc0a99bb0aff6f1224a63ad199ccbd5808ea20000000010c9\
                    d62b0e170bcfb28129070aef990ca310701e5d8342f5db000000000000a6522017ddfe44258811c1\
                    127f33fb816c20423805907d000000004823a168871dc58d42c3dd6e5ada8081269313d8d9dbdc6f\
                    630000000000684ee180223d77096648495f1a4e67f00ea66b2d2a9ea7f10000000000331c69600e\
                    e9941e12d11ece73261b697f34a280d2caefd40100000008a7c8e7e031d7bf18aca15fd8e6fa7479\
                    af53e431dfa61f1e000000010001fa400000000453e473f000000008a7c8e7e0346157f3a222942e\
                    7b09ca18b1191e4da94f098d00000000001130edfc34d4443b39a9f00c2ed3bff9aaa0213d82943b\
                    7e00000000009502f9005aa18817b4750fa8299f5ebee6b8b6d9e3eb055000000000041314cf006e\
                    c800e88237658d5639404c93b5d3316c26291b0000000002741da23472a92d503f26eca1e240a6fb\
                    ba223d9f2a2969930000000000684ee1808728658cffe7d4852ea0faad1bfab62bbcfddae0000000\
                    0022b02fdd98dcef2bf6efb1086b63f885568739e8defdca2ea401000001977420dc00979cf70f87\
                    ec2c8070d5cb12e05f2b6a466b713c000000010003f48000000043e8b024ab000001977420dc00a7\
                    0ce250ffc665dabb7af23135735daad035963200000000003fc6e780b87a2f59acf79a98fcc343c2\
                    cef20fa0c65f308900000000000eec0d58de38a3eea80b54d1d5f59b0a46713cb16e4125a6000000\
                    000138eca480de88a956b82dbb81e0aa211277da4645c963f6d800000000003ba57860df185a330a\
                    7831ccb1d7ffbf7558b5b36f5711b9000000000000989680df431199346e0ac672a2b46e5c3264e4\
                    785399410000000003aac5ed803cd841c3e82cb6c9853e2711b9343f8a0d1d77300100000056e089\
                    9360f9416d7eff1cb4b3f2e4d85e6c829ff211e65b20000000010001fa400000002b7044c9b00000\
                    0056e0899360001f087970f34a44d007e6cceb686117083122a00000000013858255d001622a6339\
                    b877221e9f2489073ffd0abecd0144000000003a35c1da800e38d2c5857e95d649d5988eb6b4e56a\
                    7afcf9ef0000000014f765f48010b6b2fee6983c03e978885c65e85ad7f40d03cb00000000003f2e\
                    5100847816309ccc320bcd7eca1ba4905a70d3b7f139010000000386abade01356cd0e7ab342e465\
                    5fc2f5c0bae98caa7a5059000000010001fa4000000001c355d6f00000000386abade01e34291d37\
                    48a0e7c0e8adf69700ad21b388efa800000000021e66fb002b60ed8e3680a7104de2de65cf51d950\
                    add0645100000000008067a4b92f3df7edaf6f7f6d460ddcacbabe67b22d50f8dd00000000000399\
                    a180329b5fd41c2360c5aa946e31672beb41f061e720000000000465e970e738139d55f67e018bbc\
                    36d5d165cdb32f444bf75d0000000043f0564aa14018afae05097ea4e5dca59cf7e6c8cb230aa941\
                    0000000000684ee180406d8beaa65ab11a6e7e86625af4ffa9aced67ec0000000004392eed1f4ac6\
                    a3e99cf3dfe1024a84cde15e12b93416d8340000000018feeefb2451bf7b66dd6e1995b7a7b33956\
                    a8632edc50ea63000000000c43ee53a06746b57c3a8ca12ccdba8e259a55d9a7c24d575200000000\
                    00e244490c933d903592fd10ddf6301d8a8a2b14fbfe6aa5b40000000001012f6a42a25e2f49e8e1\
                    0f5c88b89211fe0b1782f1639b95000000000074d33a00b17fced74e9601d1a8daa4eedeaf1389bb\
                    85a9b2000000000826299e00bd1f4fc66b136d65f173f52e9026ebb2bbebb0700000000000039018\
                    18d9a69e1efbc417661615ea974ef0788603eba9880000000010476228d3f1c58f34ef4196cce8b8\
                    9d50ce414b4b41264bd90000000006c0c859e612ed2a14135fb661a2f41ca6183cfa9c3124709900\
                    000000073bdfe52016491a68e6f551b0880254d97389bfd11bef47820000000000368fe54058ec26\
                    2aa46114f568ccd3408b86e6d48c96c74a0100000161402ac4001b46ba2eec6f025328cb873b7bce\
                    417be9863d24000000010001fa40000000b0a015620000000161402ac4002c4466840ec7f4d58cf3\
                    a415f4fed9bbb8a9a106000000000213f8b140924d35e70ea45104a6e1c34652598a6ee6995c7301\
                    00000022ecb25c002cfe57a93f73e4e553dc279fd0f27a2a100d8344000000010001fa4000000011\
                    76592e0000000022ecb25c002d68d2222674465d878da263697706e3044ad0440000000000d09dc3\
                    002dd1668ca884f664a0394ac9eb09df0e523c6f470000000000684ee1806940eec028494f91f866\
                    8b88d361d3b9c79f0bf9000000000642b2870070633dbc4307d6bb5eac4387bfb4e4c570a5536b00\
                    00000006b4583087727c4b22d6e3fe0b511ca1c3f117eccb7c242ee000000000033d1fdc00753156\
                    38cede96851108194ea1882abaf1ca22f90000000016c640884c8705adfa13a9c778e157e7bd05ce\
                    5a174c763b080000000006c088e2008748bde53f8055d4945b1cdb64236ce07f8a37710000000000\
                    37167b379293e6a7b2bae4ba30fad625d8f898b28fbfd7dd00000000311ffbe780bd0bb06fb07da2\
                    348b2cedd1c39371e234f4570100000000002ac42e60c632711656289a284af5eeb933200026f740\
                    62ae0000000005a24a8830c7370da707b4e592daacd346a0e8ffe34d5cd9e40000000018ed496dc6\
                    d18e5c165ecba2015a3c994ad250dd2a7202ffca00000000009c765240d9081efcea6917e0b329d6\
                    d6a9ce6e9a526195e600000000009fd3ed18e8d6ab384f9961ff491423f1e579df97abdf4bf10000\
                    00000081579280f8dad0c3d8d2b39a988aa1e848a3c3c61c29029f000000000067a6a718108fea23\
                    877902557e58fbf9957d76fa503bc5790000000001a13b860017f6bcdf204112e55c5f01da587cd8\
                    dec1ced8460000000000d7687895259e09a55b559d75640dc0e58ea2ed98564f137e000000000028\
                    87fa002bba64ba76522b8c4dd97b679348b66d1c4639e90000000011aa9118b645dd693b111a460b\
                    ab898656309aaff06b40392a00000000004644f26d7ac24f4081996d1acefb85558a857500662288\
                    760000000000009896807e0ee586ac467c4662e50b6c389ab7fae93c20bd0000000003565bbb357f\
                    6df1d62d759fc2687a6cd3e03e6d4085ec3c0600000000002a51bd809404aec16c18ba58f9b4f585\
                    1f339dee6442311b00000000000d180d1ba713cb338d3880fd4b75dea2e88ea0dae1bbf126000000\
                    0001ab63cbb5b777151e4036e7798273e4786e0cc498472e546800000000018c5ef280d94d054752\
                    507aacb263379eaf23773d44e1cd050000000000e6ab83800ab8c3a208e26cfb93c3f178c94aa219\
                    c373a75b0000000001794c22802aa53760874517110328c63fb7487dd22cbe6dc90000000016d8a8\
                    c8ee3b3cc3b665e3259b4d9bcbbf51a20d9f4d03d45a000000000162a5cb80453d445923b730e44d\
                    c7228f98dd00af7d7b2f3900000000009c7652406fb1db3b5d0f6fe0ac888659e7d05af26845b3f9\
                    00000000010df30e408f3ff4980426984155741cb5c25ac01e893fef620000000000173eed80aa69\
                    1fd6c697276e10efb282f4830407e7f190ca0000000000684ee180ab21a8cf03d321a0f4f1bef047\
                    0967a9587a973f000000000208715c7306d1ca4a5f77ecbddedbf16f508fa822e2c6760601000000\
                    d862a64600b5eb86935b0e0cbac951224a75475ad4a11eb037000000010001fa400000006c315323\
                    00000000d862a64600bfb5e46a02425f896373ebc922b4ab30d258c4bb000000004a21675500f814\
                    3c4e03c1eaaf2fc23041c8f52ba94533342a00000000009bce17d8fba78dd0f8b1333f38d6d65420\
                    77eb5073b5682600000000001696e480feb6c82fa567c9d2a0d81ec6391763649ec3da5300000000\
                    001ec5599007a29f1d5d97483426c6c400d2a18c2ac8522719000000000417c6198607a445d1ac98\
                    eac96b773482edff1ad587e95ca10000000001a13b860015f60cf6e4205611409d6e65c0c25bf9bf\
                    c08a100000000002098a678026cbf0b2f924f7275d0d58d07600fc3da3fcc6100000000006317bca\
                    0044a16f540e1f3e101986c98f8431240e01f9afcc0000000005e9810b9c46c29cbd8155b46ca79a\
                    869f7d3d739bc90e6fa700000000015aa16e619702351b41c15d4060cceddaf877fe2080efabaa01\
                    00000022ecb25c009d40323be257b11eb307c937cbddaaee6ae781d2000000010001fa4000000011\
                    76592e0000000022ecb25c009f9ce0c0376dfc35872ca917daad59f11d8dea5b00000000000bb65a\
                    20b0a507232c8ed1d438d96d114516203c75a88d1700000000006959e8e0b39103b390218df91266\
                    267c1864cba9febdb5a20000000003fda5a8c4bebbcef97959c9c041d4bbab2afc4e56d1161adf00\
                    000000041314cf00ca335a31e6099c0d0fc0085c1ea2535f81dc0d120000000003c905c360ce30bf\
                    2775c5273aef5eb94fb2ed183c33b332b0000000000ba43b7400accd191828032c229d4af43936a8\
                    14a0daa3a7dd01000000114f9af780d47bb196dab26a9c801d0330e6810f1c763e732e0000000100\
                    01fa4000000008a7cd7bc0000000114f9af780e02fb22e6a718f88e4de0852a4a6da6ae2d8c14700\
                    00000002098a6780e5f34a8e5ac2b1cd80c3556e532426935134460000000000006638d2c0149eac\
                    5eac9530a3778f1d334776e4db52b4b22f00000000014ace47801d6749feb50f0123c2a312c4b794\
                    f0bfea5a6c8a0000000000684ee1801e0fae453c8dc68b5096a2b4e2a71df96e1c481f0000000000\
                    b12cc02048bb19bc3051c3905b21f786acf1a0a62ad8eca8000000000032002d7058a1c4adb0dafe\
                    79b38588678668edd15a631e8800000000030109106b5e4fc0c4568329e2c71b20d1c9f833e094ee\
                    10a90000000000d0bdce206a3c0e665af1ea687fd8f6c323f7c022b9c84be4000000000049f1eec0\
                    7f0620b229c7b1437d53b62e9e44c77948aec61400000000001e854d5b9b735abf26f4f46a1f3498\
                    a6c7861507376e9b8f000000000684ee1800a624ce62530a24ed2a0c28e9f782e8bdb83fa46e0000\
                    0000f478e08400b8a13edc0aafcbfcc2b0372742a8cff54549d25c00000000005a419b9ac24d0089\
                    0e0be391b325b2b0775ded1adbe31355000000000046b4b37ecb8dd59e2d03ddeb9a5730a32c1291\
                    59053970d9010000002b00294b60d9245eb58672b3a46ad0317b8b9957b67aadee60000000010001\
                    fa40000000158014a5b00000002b00294b60db69885099752cc1ed68f78c852529495b89ac840000\
                    0000012a05f200e7b90d1ec0502c56a153bfd1667f95b6c2a5d08600000000001510f10c0becb9a2\
                    25ecebbda3c34c176478a26b677d6e0500000000008e23a2605c4084e1d93a31c4a7e5b4879d557c\
                    b4ec844510000000000158d310da64b3ab966008792b97391fd1b99c3d11bb5adb44000000000147\
                    d357006fef6c2011b7e1a21f2a483220460dba4166b8d10000000013870e34b77399de251514ae42\
                    74c531049b0390d31d33c58700000000000206cc8080fc13fd51b5813ebd2548bd2bc35b6c8b528d\
                    4d00000000003fc6e78082f28c251b90f49213f5a5c8326283524d6f2dd100000000001fa4da8e8e\
                    ad217db3ed9d93502b041358d3e453d6413555000000000138eca4809d35cfd41347bc65e7d34bf2\
                    65628eb6ce9570d500000000037646a39aa693c9f948a33357f83fcaaa286e135006ec33bb000000\
                    000001312d00af3cc56c1fbbb3e614b30ea95d71d330873bd2a800000000007bfa4800bf744ceb7e\
                    7c523faa908d9c3d6dadcf16e390fa000000000000989680c313876c03fa201ac40a2225453e0328\
                    9eb28e62000000000026be3680e5dd1e904c3ad7cb7088827ccc09aec8df418f3300000000016f1f\
                    75a0f4394ff3dd6865a4336ad0e5c3a43a0f08f1b5b3000000000055885cc058d2267e8e906c08f5\
                    fcb611416e97e0bad85cb800000000009cc29d806b7075ce85744608f7009c934c18d58f7e54764f\
                    000000000241719f6081d604d4486a66610c7a70f1c36e0be671c3e69c0000000002a18868628ac3\
                    910eb1a13279b86e3ae51342ae585d7d556e00000000021d399719a421771d0616ae2619ff919fb7\
                    a73ff0e616afe50000000000d09dc300aabb5b6b2b958202ca8d8e25183358551131b5be00000000\
                    001d4f5f7ae86aa1a882d4cd648a6ac5748ff23f11cbb690750000000000db0c0cc0e9d8ec103d68\
                    732de38d0d368d4a2606090022c500000000011501c39bf70f7a575b3cc4ea3ef70af8531f4aa525\
                    fb8519000000000007d2b750fd196f0aecfe3c9ef7582c00f72b1ff0de5d3c63000000000004cace\
                    80feb44b39a5f627a1a3d0c1ed1692d6445757338e000000000163a3c874015362ae1c5b0df1b467\
                    5f52a0fea3cda3442a9200000000006743da2015bd67cde53a8a6c863b8c303b2af26fb008853800\
                    00000001da3d72fd1c0845c0295f0b093ed5ca9e3d342a9d8600fbce00000000007d2b75001c155c\
                    9d410fc8d0b159fa5ec647bab0288ba0e300000001be652b3dbb1e45fb5d6ebd6ea6582624fa9a33\
                    87440348ed6a000000000217c597c020e7aa1ff5b5d7573c5cf47d44352f75c0e84eea0000000007\
                    045559542c9ba31a1fd8d50949f760481b3f97cd58cbb39d000000006f2215e49a2f58f61c195cde\
                    f2633ff7339678e91f181e606f000000001d210dda293daa006935df179acd7fb3f04e38ff4bfac1\
                    b65d00000000094c9c09003ee68231326fb98ed5debf153ffffd3e07aa210800000000003e95ba80\
                    4c99ee96cddcb956e6063266072e64a3274776c90000000007d99d37945289e83f2726e01a046832\
                    9ab08e440734b58177000000000376fb3a515bd2229f716f1c6b094c6391b508ac640f8a8b190000\
                    000000166b8b9468bda0f9f72b099c2b9b1fb9eaad84e426d800d900000000032d01e2006a0f76c1\
                    32f5158869616b634dca7a30505379dd000000000f377c524d9b68de54ae3df1876b234bb5ba40d6\
                    a61cf5cd7300000000009c765240a6ac0a1da8c64658eced64b9548a96cd2462d001000000000c39\
                    3e6d00c0b349e2164c2627cbe217dafbdd903586a3603500000000000bebc200c3ff439e167f2bba\
                    5be803240ac13d04c6fa8dbc00000000006529858fdab92792d5132857ff09b763c6057700be859e\
                    a70000000000146a22a0dc079b490b93270da8e46f4589130d6f1bea319a0000000001a8629400df\
                    20d00785d420d85e568e66560ecae0a7770b20000000000017dd9e80e26f8fb893f14e55d774d088\
                    f23281b32402e95a00000000062802915bfc463c561dfd3f6925746f49a55b5f1911dc091e000000\
                    00005335664c417cee58074efca8cc78f6a4fb1f9ab22f3d6f320000000000342770c04318c9224b\
                    8cb2bcf20e6e9109a17a8fc0946ab700000000028c8bde628283872c745aa7331862bd32247c8cb5\
                    c8a859ef00000000003c7fabc08ccc904a06f37e35404cbf306839b0fd211a233900000000001add\
                    e902be50e822da68f16a96a11bfd803b4843e758cda3000000000138eca480e72424487c166f5f12\
                    ab999722caecb5b106b15b000000000303e15180eb09c3213d59e90d6241839a957fb76988bc07a2\
                    000000001982ac1ca0f2a228457d9800b7055a273d19e8426a1bb1c81c0000000000342770c00a1a\
                    deadf77d08a370ae54cc8027d7aa7e00da5c0000000000009896802b3419fe24a0cd5d8713ba11be\
                    b2812fb2477d9400000000000566724039873bb3e1ed6ba0d3ae6030967584491664bc6e00000000\
                    002d2e29804599cfbe62fce3a5efc3c78981233493114377ac00000000056ebe8d615ab6457201ff\
                    7fcc7a7a439b2dfc57977fb721dd00000000000ff15e608159e684829d7c013a61eb685667ee66ce\
                    78c15200000000000a6385cfa185d1afe65fad9ec07bedcca30361bc9bb72ff40000000000a929f8\
                    20a421ba4401945c6fdbaf3fc60f02723e5fb6a213000000000540d2e700b71501166f110977f647\
                    766fcf18c9cd8470c1620000000001080347c0bd9c1be2ddd9124813d144df1faf4e8e00ad070a00\
                    0000000138eca480c899738c4c5117f9d8bb057980635bffb366891d000000000000989680cc25f6\
                    ea67e274e878c7cc81e623cb42aa453f6e0000000001c9a68220d45690792774ddd2862c42aefae8\
                    b49535d14049000000000ee4e2a200e15a7c3bdf8d4dcfca96d25a15ffdff971eb62f00000000000\
                    95a31acee2c384cee8d8ee0dbc2d2b5d7d4f6d4578fef25c000000000068e77800f61048116772fa\
                    faaad735c07663ba08b4f0a7ab00000000046033daa40b08f781be42240c4c36a0d6a590e7b1e820\
                    a834000000000025c705401e605e6acc237684d8879276d556a44374e308000000000000157b4480\
                    3740d361fd9887e33528630b9d2b76d3fbd69c040000000000bb5af3a04bcb6b3cef75893a964a83\
                    5aa888772da82920540000000000903e450066814f57cc55b631df64c6098fadd5ab78c18cfa0000\
                    0000000a6e49c0708ee7f6cb5809213c63dbb7d318e7d031e4641b000000000064dc5cae93dfbd91\
                    b06e75d080b58ae71d6f4722fb7f770500000000003d3707b0c480344b374f8cae2db4d70304554d\
                    b7dd7ad2af0000000000d09dc300c9da437b5a8b907eed55de12bae2af2221d0ae50000000000155\
                    279bbace8b711b3f2103c476346ae7bc6fa062f9d8733b000000000447688931e321aa13adaddab1\
                    ac6201fcf0d75ec1d904a8ea000000000ef613e9831e349ecc1ef812bef6fc1fe64c0ad419ba3ddb\
                    7a0000000005fac66af71f13ac234a8a6342528316bd05c9b31fa72f2a2b00000000025706d48021\
                    6428c0ec1d962a5c87aa5e0fc8b2a5d88b9fe8000000000716f620802c45699520b8e13e186692e3\
                    c3e2c7110d1e082e0000000000a3d1f1a02e131b02de1376e3e764c5e3cc9e8652b47cef0e000000\
                    000520cb099f61c4b145f367f8c21f611840dfade99ef7fd33ee000000000194fd3cac655d569900\
                    f3050f9d66be4d5755007dc674a2a800000000004190ab009806d20f71d6db86947940028fd6eb2d\
                    f69031bf0000000001213eac229e86758accd7d83115dda28e21166cdeb34893d000000000006377\
                    9d469fe8607d0ca8e96bf1f294c1424c5e083e9ec1d500000000003f2e5100bfb0756435c0e5a939\
                    6c4411a54f314453bf43390000000000fa56ea00d4e08315958443a76fe5237a36743ae89fec4ff0\
                    000000007a3c70420008783a1a6fe2526b24c09a63a21582b237cefdd5000000000e36447c003794\
                    14478091d38da882f07aa11ca8457f9d8466000000000b1584fc5a31e95398b7e11565c61cf609d8\
                    b940b3c18d7e77010000001176592e004756335ede8094b859defdd937c721eeee7813e900000001\
                    0001fa4000000008bb2c97000000001176592e004bf17122ddef3091ed711ff204d4074e9a3e887b\
                    000000000258da88436c6a771c60fa8f7f5242a1b83c8f98e266f1bfc900000000041fc2c07586dc\
                    278acdd1a2a62fefc98f167cba40f35e09c00000000002034a9346a83f41924c7230902283e032b5\
                    04e2da6c14aa6d000000000188683ce0b95797edce7e767f31356bf4808c403337dd17b600000000\
                    2095f7d2c0ce1a7ce31f1a9e55ebf865a43e539ed4ab143b3e000000000029b92700e6e89076f0b9\
                    2627b0fbab622ce208d255032c9300000000041314cf00c8b99c3cd4a33df368d96e2666d929776b\
                    cb8aff01000001319718a500fcb25eb488484eeb7fd04db94dffe264bd505430000000010001fa40\
                    00000098cb8c5280000001319718a5000a76d33185e98fcacef0fc4600f82a311f5b9f7300000000\
                    0007b0009851c17dcc71479fdfc2464ef68816f6a53e16693f00000000009c7652402caa80bf426b\
                    dc19dc9c00d9a43e2da6350fc73d0100000015b822c68062a153a5eeabe57af690496251d9c23041\
                    68db50000000010001fa400000000adc11634000000015b822c6807cbb31f375be7d029e60c5882e\
                    08a0e32dd9c7120000000004612ce8a98b0fa0da24dc92090239099dc51573942a6767b700000000\
                    017797bbc09eb4d34727e07713836cd10f8711aa12e64bbbd70000000000684ee180af95e4a115fc\
                    c2e3a7f3adb05eaef2485dd67dae00000000006743da20d204dffee9638e49e48817fb10a15509cb\
                    e2494500000000000f273884d309da8a2bf1e1c4efe48b698d3cc7d1eef1a1b4000000000138eca4\
                    80dff0924886e52b1f32721ae751f28784902c7f7c00000000002e6f0d33fd911c415dd62a96863e\
                    b956191ac84f832db5470000000000671db4801ac14d6eeab009a34dcd9bc41eef208d9384873c00\
                    000000000a6e49c01ae27d72c54811d772a4665425152c6ad640b6760000000003aac5ed80308d6d\
                    d2a9230f94ee3f9efb19d32eb40b47ea7f00000000004e9e57c030ae8c4e8f0bfc87a95d56e4181b\
                    72c3292c73e70000000013dc7b460a42d0e405151ac8ffae8eeb7ac75f02e8cfc8a5490000000000\
                    2c54b9704d3f6a252e0df4422f0fb485780fb1abfbdd8a7f0000000000157b44805c52965dd761cc\
                    e4191c3f724ca2ea93cdd093de0000000000684ee180616da4072390222ca4f74df29968e20e843a\
                    96370000000000342770c06a46940ae1c4d86f350ff016540b5155b081ad96000000001e1d71da2a\
                    770eb3b3db4f655af6452d1520464a6490fa95f2000000000043a4577596b127d9052ac7077b415c\
                    6a8cc9dce56f76566300000000002cb41780ac27f53e6ed8c9ba980c477baada6eac335dcbfb0000\
                    00000000989680af3fd0751a672560e49a1a17848374f31a3035500000000001ee1f01cdc797ab0f\
                    d79b61042189d947aed5ce7437b975ab0000000005d06148ebd2985450804dbdfdd9cfe5858b158f\
                    7a70126b600000000000684ee180d68e7ae79d1f76663e83ef7cebd180481837b272000000000084\
                    e2365ef9f97cb91a830a2720707301809d468c20c89506000000000011bb7d600fd3a8a421cd8274\
                    0d63f220d968e39ef5215c3700000000004ff5aa604ab24e1416cbd6fb836085aa64a95e37caff8a\
                    7700000000185c41e02f5da341e30e77620ce22c827ea745e55323f2adc90000000009f24f780070\
                    2a534fa0ac4d77362ff4bb7a06d381f04be1c1000000000640dd42f48ff16867659e27933fd4a182\
                    90268839d7f7d119000000000068f454f3a841e20f1adb47d40ce7171bd39dcd580583d218000000\
                    0001a13b8600a9d75b982ef5f2e9efbb77b7e179f889757e366a00000000027dc50b00cf86a6b263\
                    7ac615d35940a6bb139ac4148c745d0000000000d0763dd0f1e052699c2d1bcf8dcb87ff0e96b213\
                    c988959d000000000035b7fbd0f92277008230b2cb4ce77e332dae9639579f868c000000002dcb9c\
                    64972f6bbeffe7306cf1a5eafc1ba5c039e5d5474bbe010000002b70458d0007d7ce4b19b795254f\
                    5109c210ff892f95748a37000000010001fa4000000015b822c6800000002b70458d001c9d7ff28d\
                    a41d42b5ae0f0871131bec78be7fc7000000000006c7aff02694f77ef0b6c830b1518861156ead74\
                    42d3e65f00000000037892912a2da3183636aae21c2710b5bd4486903f8541fb8000000000096004\
                    4c00371fe03628f6146a45dcae22ce010b4bdc7249c20000000002315cec8356a3d0dd4a7fd68553\
                    b1d842511316ab19ac9e32000000000024b6324b5fcf23ad86c4e7c2caef971878be3b5eba66d5a6\
                    00000000014920f52974695248c5798101527fe8d6018a7c52d834c1af000000000030c655a9850d\
                    3b4a43cab895562e509af0fc375c53b934320000000013e23c9600a81796f7a36281cbfed9cadeb4\
                    1e13b141f6de26000000000000989680b225327faf32dd0952f6d380b5dc7bc01a153da300000000\
                    0ba271b080c640ffe14a4f476294942262722d78a98063d7ae00000000003f66454cc6441ce8e60a\
                    6da8fd50a62778e620c340cffe5a0000000002305bf6f8d9a7ff0827bac19ad7da9472dc8e607021\
                    f56cdf000000000173f162a1dc2c1058682ad9e2b75eb5a6b77eaa6e7fbcf72b00000000008799be\
                    c0f9cf2e9fffecf760b73fc89d98cb50a4000f7a55000000001870b31680fd4567097059497f7120\
                    18155932135cbc2365f7000000000a31058bc0855fd96b13cc98443cbbb577fc14762d4c492e1a01\
                    0000000132a8f1a007397174290a1a587fe955e0e4f59af37e5aab87000000010001fa4000000000\
                    995478d00000000132a8f1a0171866db6d9d80f61084ae41e382055d1835f2d1000000000019f071\
                    0720cc540863e620a639a73c243830a8310ab3dbda0000000018dacbbb80f6afd3ee3e1243b301bb\
                    a454e14ed7f0f869759601000001977420dc004dcf0b0c70486431227b32eeb42a2f6ca30c5b9a00\
                    0000010003f48000000043e8b024ab000001977420dc005ee2ff77542f63cf46177f9ce64f1ddde1\
                    ca79e800000000226c90515f66a8ce4130803e50da5e7f21ee1b8c9f2723588d0000000055079dc4\
                    006cf83128db5e097319677253dc523b6ebb28cc3d000000000057516ab071669d69b1a4095161ea\
                    04e6e62e147a347a1b4600000000187b6dab807a2571238884819a616fcd5dbf7bca74142e6b2d00\
                    0000000bc50b535d828e88e00dbc3a735eaf6804749eb5a7515a69d600000000006d6d8d62855a07\
                    b67a4c77f6e307aa1a4b8b762b2c0db78900000000005e60c4409ea8e6935a9e37882e783ed5653c\
                    40a1a7cd0ddb0000000057e8055f2dc6e97f003afdecf5e0afeefb14ded2c29cf48ccb0000000004\
                    a9dccf97cc4136afdb85127d02d777728614052006ec91250000000003758c63fdd3c0f08a7d7a48\
                    34b1e97313be6e5b360356569200000000004c4b4000dac3b4baae81169d5993c1fa997e690522ec\
                    873c0000000001ff1c1dc0e8a5ff5b5264ab0d81904d2d771cadcdd60f299e00000000001f4add40\
                    f896825d830a104a217a988516b55195a5334ff2000000000072f10c800998726a35e16f2c810229\
                    65bdd6395a3aae58fa00000000020a956ee00a5f8d99dc12e94079486cd1147acf259f2d426d0000\
                    0000011e1a30001576a2558b85545ecd0d8d0b568d366cd2044e160000000000d09dc300160d867a\
                    17d2030f4caa9675fc7066ec88b0f0b30000000000009896802c958dcc325e5e63917d159d9a0ca6\
                    786adcab54000000000079bc3670461cee2bc01b07dfa43153be2c514e9a600add9100000000026f\
                    76ef005800a28f318ff464e1d3fdcf833afe6efe9662b300000000004e3b292059333e9560a4a009\
                    8c47a02a0df41cae36c6b5c7000000000040dfaa80729d41e71fcdd403dc2147a1992f52fac2f2d0\
                    0a0000000675f026928077fe71b45cc2ce664922b16381f1e183282affd10000000001b23c68b87b\
                    2b8b80d3fd85f5c7b54bbe7c77513989c3990500000000262c1b172385c01b33ef589086b192d06b\
                    33a4338eed43ac9200000000000098968096af4324a391339cc130952f769d98b716889a02000000\
                    005a5ba24e9f98f5767b7fe9091f7146ef067dbc06860780997c0000000003c5a8a5cdbe3c910763\
                    ce0cc90e6bbeb486226fb99b1b0d9c000000000001312d00e30c9bf570d152f87ccf23c113eb49ac\
                    767420e000000000041314cf00e5b5ea289911c0a7be65b3b1cac39cf91f517fed00000000179b5e\
                    5b20923090173eda76f373447bf38fd43bb82543b410010000008bb2c9700008a618d31b7fd6c407\
                    9e24db5898071b9670419d000000010001fa4000000045d964b8000000008bb2c970001232e919cf\
                    237c81aa13a9a6ef3087c04f786663000000000130dbc4701deb8cfa143ba9975abd912a19754235\
                    605331560000000002540be4001f56582c8b4a16f77e27bc7cad7aa60c2491be8d0000000000684e\
                    e1803304ef4a109afc50a7ad2b2d87d2bbc17cfa8258000000000038ae05473e3f5d74be8344a11f\
                    8d9602082a23e2444e618e000000000053724e0044c023c3cce3db087d091c5f608c5d7f919efe90\
                    0000000000684ee1804c4055cf472a5424018b2aa1ded37df886965907000000014256290c405613\
                    0ff25afc1d6a55fc84251fa5a7cd0e69ff1000000000009af8da005a756d3171624e66c8ce0b68f4\
                    5740f95d28bb070000000000659af7b76194fb672bae99e860134cdece86418d5d17fb3000000000\
                    009c7652406cf34c7e17fa51c3eb3d69fec18cdb5dda2e5d7d00000000019095be608082b9cbbaa3\
                    c10d119657f80fe5f42baf877e5200000000008320547b2559a5d59fbf45ec48233218c99eb88ea0\
                    e642240100000056e0899360a0049600fecbc7b3312ca0050d6605a67fd8d507000000010001fa40\
                    0000002b7044c9b000000056e0899360abab9760f958a163eaaffd99e98d966266a31fcd00000000\
                    012b27dcc0b0e5dc9710715ac15150f904a50e81dd3622139f000000000005f5e100b2dfd0040a47\
                    5c1d558513a37ad15ffca5584ea6000000000052fe73d6bb722b5a4e5f70b30cc8617888ebdb96f5\
                    f235e00000000006ec7319e20531ce072255bd3de8e252ce25d8312d30216763010000008bb2c970\
                    00c55113641286f254766b44e6808b8787ba4a1fce000000010001fa4000000045d964b800000000\
                    8bb2c97000c8d592340b64d1a34a2ea168a2d0bf605a6175fc00000000001ec18900c90af9e93a19\
                    5ac8078e8c937f750444dbcf4c6100000000067c7b9647cd9c259d40fd7cae4bfe016410f1c81528\
                    e5e9ab0000000004a817c800d9579ac56b1cd6fff6f123d09db964254b6eb1180000000028bed016\
                    00ec6894f7a2cde8abbf56524b3d4cbcc58a5a3bfd0000000000730f9100f6a4dbeb7de221ce2da6\
                    39d3d782798c36bd0ed00000000002efccaaf02dff455cfc98717cc7bc81b471c0fd8152295f3300\
                    000000090e7be410371b8d893b295960ab9d1e05716f944e692fe27900000000073c2ee68f576f09\
                    bde517dd032f26c2a2cb0fb19f6b86873d0000000000bf3a64385b2db4f81f878f6da0de3ba904c3\
                    11e16c811dee0000000000c9369ec08dd79341ac24b55423a017a6c712c828be79f6b10100000074\
                    6a5288006099016973f25367585e3f94658bd64f01a7fb0e000000010001fa400000003a35294400\
                    000000746a528800b2451f3e949dc0505845c559a12064edb584e61c00000000056a014b94be878f\
                    01f45c7137f79af0a951f59905fbc6ee9a000000000008f0d180f0807c70ccd87a2bc826a7cb298b\
                    2214e6d4476800000000140ae953fc195274b2a44d415487cc8496be7771696b86a7dc0000000000\
                    3e51312223a6e0f3ac4d47291adc8544fa7b0564d0eb2b3600000000080b06f3f5947a887d9206f9\
                    81294611953a9d2b9418e5e389010000002b46ee0e0058ef72017a95654210f7cbffe791b3943032\
                    ce5d000000010001fa4000000015a37707000000002b46ee0e0059e312f3202b26a692e40b0901bd\
                    91101a53eab400000000061c9f368065dd0ac968fc36314e82908492091f3956e0568e0000000008\
                    05453c2e77bde99d4af9c3671f07eb9532246116f9281ba600000000003356351fc6bb56f6c977b3\
                    123eb5a095717b31e48825a24e0000000002098a6780028d8d85887c427b6a8a7cf70638385c3d60\
                    bf0f00000000047b63b08015f1d7eb32ae58bb9537ea36e745a866d7ea72ea0000000000671920a0\
                    2f73719f43aab9989ecd94e61a130e52295d2e70000000000008f0d1803a5c3fd56576b0f9fec1d3\
                    a0d4f26234e670b06c000000000014dc938069549f3460db21b71645775771787e87d4410edc0000\
                    0000000835844892174b36712b9b1dc35c1b5796d7656c72ac13530000000000025b6da194aa3d34\
                    8815b1e1a1d8fc110a4706892784ebdd000000001d759781b8a42fa062370c222d07d76d52d31ec5\
                    d85ca8cddd0000000028bed01600b7f991d7404bfd3ac2744bee76d40ad44cfa61a60000000000ca\
                    82e3c8c84a60cab65e958965cb5987d2e9d80baf7927f90000000001fc5e3640d1d1ecf309396c6b\
                    4d50dc61938e90e0778dc2e10000000000684ee180dc1c53621b66775a89c6d99e41d68e11388af9\
                    d70000000000ff1a13a94c757a90ee9b11a2fcae2c693fa5f98ec93dd7bb00000000002e6fb2595d\
                    1a3d7073ee30c1ca566085c1e43bd7c54d7e500000000000684ee1807db21674937ea0fd357d962b\
                    8915dfdfd030f11f0000000000e612ed00a4262ef1540b0ebefa034ae7dba3a391f12f1f02000000\
                    00041314cf00b46ba513f472fa85b60c710ef010e38262e4562a000000000179e754a2c6a7a8fccb\
                    21361c3df867b6137c6b187c117ebe000000000118464073f8c92884bb21fec24b45f19d876264ec\
                    c6380327000000000128c1b2300524afbc1ab0b0a9023b2d3e765511c9329e1a2900000000dbd714\
                    77103f870a7e36ac4bb0e538036e6553a36550d167f700000000031c4332c5473a0e84b27c3b6147\
                    9f856481842ce1b427fdd4000000000342770c0055d38b0176bb4339ae483d2e48870086a0ef0636\
                    000000000128d4c50059cd5d81518acbcb54a93d14af1ec148c8b4c402000000000826299e006036\
                    66c39097fd3af7897be9e0ab43914b073a660000000004d9219188a5b7980b9d3598e69618555e56\
                    02a560ab5b14ce000000000b85b3e6c0d711633fb4ddbfda46109e2e611cae53e355c48b00000000\
                    0b2c385844db5851ad09ebd6cafc37b05acf7e336c9672cdb7000000000474f8d505f2b9eab8266d\
                    d4f44bf44cbae19f1af19940c85a00000000041314cf00f95f281a92bd8b4ecf6cd4e91e34678acb\
                    ee228a0000000001a1608d9b37f06a00a49ef5eebbe6165341eaf0381864ddc90000000000d09dc3\
                    0057207d845c1fb41b2e2b05462a7f92b8a4a1286000000000002ed557ad6f99c7d05b145a67e3de\
                    f96fbd3525a7f61fff5a00000000001b1fd09a8c92ea3bbfec5d955b2167cbdbd71d1503368b5800\
                    00000019ab697e809f64c0fed218a09ec502a27897adfd7403d9af27000000003b9b629680a4018b\
                    2eb1534e275db76266e4b979cad7e37bf000000000006674951aa78acae58b6f85929f8d5f06065f\
                    f9fced6f3270000000000826299e00aa57ee0f5fefdb205066ad8577b3459394e57c3f0000000007\
                    9ffebc7ac2949b49bb1bb6662526294560581df2c5f2ff080000000074db0204f0da3c97ea1103e8\
                    b8f6b63313c72d3ddda30394e60000000000861c4acddc319d79be03860fce49781086e4d7ca0244\
                    9df00000000000684ee180e6da972de1ba77d07032f2159a27214a6cc7b2f10000000000684ee180\
                    ef2f0e942f8ddc1585af222a9184a4a1d056a0b9000000000055ebe5c9f655fe048ad1e49915ce93\
                    3297f7d33979e5a177000000000029b92700fff92af40c17d8990c5c0b5b20bf73508389033c0000\
                    0000007d09dc7f004df512449111ab60b9721f15bb26de484f4312000000001e4778f9720920d09c\
                    f2705c5ccce40d17efed9bd401bfbd410000000002ff9797460b28da77fac452321aaad99eb94512\
                    41fa1204ea00000000019963778015464137648777f03a89799bba3e70f78284c33e00000000053f\
                    431f401e4a358b65d421323faa79d0728fd4c4f19a5b480000000002f692dc893f09f88c7f1c185f\
                    fc7d0f28433d1ab2b2fb735000000000002a3339005d6185072976bd221cc699df25658ef323eb27\
                    64000000018bcfe568007c991feced4109e80b6b4e00d4be4f2db27a820c0000000000675e8e108b\
                    8bb6c260892bc62fd0cd4ab11d15560a5c0004000000003b5fc7cc80990467354749583950268f1b\
                    dc932baf2e1f2eb40000000000655a0b8099be78ef96b3f1dcaee88d9c737e966f2f29de71010000\
                    01977420dc00b5326ac47f375fb9dcb924228c49b7938bb37770000000010003f48000000043e8b0\
                    24ab000001977420dc00c72bbb3ec6bd50f678b45f567c497517737ac0e70000000000684ee18091\
                    84a309e12d4e9b88454e6e642bbdd4eefe3f6801000000037e11d600db9c6ae5094593a7f48e6b66\
                    9f70603a420db5c8000000010001fa4000000001bf08eb00000000037e11d600f0f24a15c96c3b7a\
                    c077a0b2e03ec0a588220cff000000000d25f146c9285e0c24c9f50157b2ed7b7a9f1611efc3bfe9\
                    a20000000000630916a02ff5ba846d62306c8e23b803248a4d53be5b6a440000000000961033e341\
                    99a8dff10509386b2aa176e6381977f1663764000000000af3d1f0e4475677807c9ad517f47eb719\
                    3937c5464ecbdef00000000000342770c04f2db8eddb9837bd8a356b01de605326945f5d44000000\
                    0002a64871204f63b41216bab1331d4e2b86d11814d8d558ca220000000007f8c82dcd57b314ecef\
                    60c8b6f2e2c1c7a768b9f7f40657e7000000000029b92700fc3687f91b8bbb1cc24b55dd640e3c9b\
                    b23261a901000000098e4210c063ddd9ab4e5d3fb7b41ff2a06647beca2fa1f52a000000010001fa\
                    4000000004c7210860000000098e4210c069d5d63653970853e2b019febeae72e8ddd600a5000000\
                    0000009896806bad6921fb455361f6a972ebb3de0a6d67e4a4f0000000000002faf08082a59473c9\
                    b1cbc0a3f119dfd9d39445d57b12b2000000000290451f0a89306b4189780061c721a43aa5f1801b\
                    72063a9d00000000018261a0ea8ae5758de38258ae9783ed616b9e19590fbe07440000000002da28\
                    2a80924eddc10023be35b70baae59de13731dbd88e050000000000e2149640ac7dbdeb3d9ad56fae\
                    7f1e36338fa0870de282fe0000000000bb9c17a2ae715f499557f5a66df7d3d395ca0f82b5adf7e0\
                    000000000000989680cf483a48691a10512f9e4582f5a8f0007fd0b9a600000000021353e5c0e2dc\
                    37feb51b146a86ff453815c63f4ae4c24baf00000000052c59822034991cc7d5f6d35923e101e198\
                    8e8a6361d13749010000000ba43b7400ec567774bc40ae5b832b378d8791f4d5660d33a000000001\
                    0001fa4000000005d21dba000000000ba43b7400efc5501237a26fc4957229aa0f6a81ed1b296e54\
                    0000000000202fbf00f6c7af0f8c6213900235f95c00745cab0c843769000000000615f4914ef7c8\
                    034bbad7a6f62f44ed228e8fe120ea3f4b03000000000001312d00038227c6e91cc32144c164a1ba\
                    2183f7256e5767000000002618cf5c402813a2ffde3aacb03390aee56acdcfd48d009a1a00000000\
                    02098a678030d2b4f935846f2600921c19ee291a9255e0adfe000000013de0c9c4915328f6604fd7\
                    2ec38a2ea672bf3790c3b95e633900000000000988a6ef7a5e35367157e434b1f3c03e5632adf41f\
                    73b2f600000000cfbafa68d981ccb8bd45c57366fc6f9a1a8af291de3bfda4b80000000002ce0e11\
                    f0abff0b570a00ee4592467ee170d76aee885d084000000000038dfc0e00ad038da0314cc101853f\
                    fdd3b17dda792f62edc8000000000204734734ae4ca15fbed7b878c7d727703e78150db50bb7fe00\
                    00000000d060fea9cc74f9fc0794fd3bdffc3b504b55893c4685488f0000000000150cc9dfcd82c3\
                    7907f1a05c9cfdb76b5511a40e94dbef3700000000007cc0a540b11d3074bce47572db0e1f06ca84\
                    4655b1b6c90c0100000022ecb25c00d1336297b4f76d04c2921ea91dfef23290dea79a0000000100\
                    01fa400000001176592e0000000022ecb25c00d1fa92461104000909d78eefcf99fceeded75dda00\
                    000000041314cf00d722afe540f32e0152bb65df7b14f8b0d2bc1caf0000000004f4630800887a8a\
                    093d0b4bd862b741ec26ba8bf5572618160100000022ecb25c00ddf7adb0d0a547af45fd75f5baaa\
                    015d20a1bf59000000010001fa400000001176592e0000000022ecb25c00ec956180dcf3771ca4a1\
                    2d1a851432a9604c8804000000000c2d9ea17ffc660ebc35d4de3dae65df744d2feb25d315e7d400\
                    00000012a05f20003eab11d3253c2d3ab30b22a5d1a5bf4c637e4ccb0000000000684ee180410893\
                    2ca62d1ae1efe1443852d7e928d5c778fc0000000002218808f46741c9aaa1ef26427145a669599f\
                    d09d4a74719e000000000014dc93806d28b4f435b2fe77a2b9392d908101dc6cc514c40000000000\
                    0dbba0006e47bfb7cdf1f2ea32eabc8a26ec97770bad4bba0000000005997de0809523569da9361f\
                    27d45d8bb01c46e5a52af355490000000000d09dc30098554f526e1b4c2f771f1d5a503b46ac7ff1\
                    9bf40000000000b68a0aa09d6ca05782381c03743752a33bea339423041fcf0000000000935cdf45\
                    50caa3f93655b5490ba368e19eaf8cad0a9cd0c80100000011601c2020ae26b0807ec8dd46eecc06\
                    575d8ff974f3435405000000010001fa4000000008b00e101000000011601c2020b43d58ef11b5e3\
                    099c45ebdc1aed387740e1ddda000000000178c1cf53b5d270b619fb0ef41f34a8e9e142daad4777\
                    dd6000000000012d49548ad7014cf30595fe93dbff64d75f95be9cef768eda000000000826299e00\
                    e476a59c63543b674cfc0af6f63f54bfd929b1d1000000000fe3b80f40e70ae916fdaaa831da4889\
                    32d5c5a55c87b8959300000000623ad9855cf7d4aae4f87215652a5d5207e0f6aa896cdaee570000\
                    0000009c765240f8159ae32f0a9ad692d43ff362dabe6d41c4747f00000000001c29c72019124f35\
                    9b365385e513a4bd20ba5c2809b88ba20000000002048666622b723459ad70939a3ce3f35b39a7f4\
                    224c31dcac0000000000fd1aab7d58b61ac624552e422cab4bc7d7a7b648f6e364df000000001526\
                    13922162fbc9bb7a3d572c684dc1e6b19ae0e1e54e1ed50000000000f492a0a67a16b5d0f196870f\
                    710b55ea4eded129f6282821000000000138446a1883a23ddcff584a89152b92fedafd97eb0ccec0\
                    490000000000f171b1698e1439e55e0875f829929efa9e9cc88b924fd8080000000001682d0c30ab\
                    453bfa23f44ab575ab5f85dd56174f55b2065f0000000001d2445c2eb66094fd82d922141f54f55e\
                    d9dd32e5dcfc699d00000000003823b6d1d6227fdb956f50146298229b81bda363fd5d0ed5000000\
                    0004795c5c535932b8c7192412897befebf40617894be9d84e81010000065dd0837000df44962e6c\
                    54d567c6bf3c06bb55271b29ff6733000000010003f4800000010fa2c092ab0000065dd0837000ee\
                    69724ed8df0242dff6069c07d0db37399644d000000000146006bc00fd0caeb9b784f6df7d228a57\
                    8205d043752aa07b0000000003ffb7d32febcbf0de7dae6a42d1c12967db9b2287bf2f7f0f010000\
                    2fbf9bd9c800fd34ab7265a0e48c454ccbf4c9c61dfdf68f9a22000000010003f480000002632e31\
                    4a0000002fbf9bd9c80004f198ed235b8a12c5446f18dcadbb707b8ab96c00000000003a82f35905\
                    06cc579d24ebd3b7a7f07a2c26613f6410b37e0000000009844ab02706b5e6e9011da3febfc7e950\
                    e0960a387f48cdc700000000005778fa6e18f90a199048e0d7ad72647b7fc6ec7d57729199000000\
                    00006103370d5700fdafb9442b7c6e0146d38335ffb79d67244e0000000004d69a86cb5f5235a35a\
                    f51c53ba988528501a0bf41162ee970000000005330a16771b9d98f10c9bedfca0b2275e187fffcd\
                    2ef03b020100000005d21dba006a90aad1aec0259401e0ef24ef08d9890f4c74cf000000010001fa\
                    4000000002e90edd0000000005d21dba00b9b7a609042dafb8e71f0aa97856441601f0b716010000\
                    00183bbf2e008f37993974111be44c00ae51bcf51679673f4819000000010001fa400000000c1ddf\
                    9700000000183bbf2e00913f9d43156ac22223d8417a94177aea6a016c5d000000000008f0d180c6\
                    8f4b5bc1e147f39d0f0d0b0784ca4cae2109ed00000000054c017380c983a8a20b69c3e7d31a0f90\
                    0f32b33ce9f5b9790000000001cd6faf38fbe040f47f707ab55a7c988746550c6a9cbf4187000000\
                    0004eb215760fe0212a71ad52673f83435d5d7a52d6a878f5a7e00000000005b89ed2a02adf7a2c8\
                    4ca2b82290a546e2b77d624e67535b0000000000c5c5714703b1a0346440823ef12090687c9c2f20\
                    6a7eca3d0000000001c3be5cc08d692a48c3b21b5b2846166aee6e0cffe4d5bb8f00000000002376\
                    fac0968fe2cfac463f69e5731bbc2c283fe1cb8393ad00000000002cb59d86ae0bdc606e6c1e143b\
                    c9d00fbff6019e44c1b02100000000046be46780b5a2d1bfd78b8f7a51e708f41e1d4162aee54664\
                    00000000001a7b0112c5bb024473a999c7f87d2604aa2fc8b3ea8ab99100000000035ef76b18c5ee\
                    82deec18981e96c782e7a9a27894486c4c6d0000000004b4de5711db0e1fce4eb40ea0a4ee18b35d\
                    4ef11c78b991760000000002098a6780e3f2f0ddb7856c405a70c470afe498ac73e3296f00000000\
                    04127c3880ef30b02b4ff0829e7ef493a11d6167535eb0c802000000000274188aa0135434e1e92c\
                    732274d09d28da61337c1751e83d0000000000341fcfa016bc48d87aa4f68119f5c1fff175ed5d47\
                    733c810000000011133d2fce3a3531ae7dace65853cc20d4a31fc2e6f31d18800000000000b71c1c\
                    136047af576890cec8380216c29ad2a6dfcd79129700000000008384be4678e5c138fe4c56e4247d\
                    c666402f5aca41c038a60000000002676aff408a7162d347f72bebc35b1fae91f7779318ea787c00\
                    00000000fd1cac798a91aa161037a0b2f5302d4a40a398a25f6a8243000000000342770c009871b5\
                    607c073ebe9c0bc8fdceb7e2591497ff71000000000005f5e100b2e37361cdd6d437c45d732030b9\
                    5de59b0ef97500000000000d0a9fecbf0ff8f44d261ad7355efb7c4210ac9003d580410000000005\
                    c2485d60c26159a74cc68cfc7ec2e8fd43bfb001d975fcf300000000006743da20cef3bd7632ae0e\
                    9a806845395a24785d8057e6dc0000000004633ac4bae3ca2f53b38a3075f2ec7489a7516795e997\
                    dc03000000000602a44829f825839e6dbbc87f847a37bf42a487d4db20710d000000000000989680\
                    fd6cf7b9a81138f8c33cfe1f7301a6857569756700000000012de32faf0f85d7680d3bf451fdad32\
                    56031e3c8ef02023470000000003b7b9153e101921f951ced6c788ff74f1f20405141d70739a0000\
                    0000002df4f510f5f84e21e5953d45f6a20b5fd7b65d50e23fca0001000001319718a50011339ad3\
                    d1fbb5f1b7637fb1a8a725845e7fa7bd000000010001fa4000000098cb8c5280000001319718a500\
                    243cd73467386ef32b31f8cde0b10b3cc3f4887400000000012a8004002c02007a026949d5a40eed\
                    4a7874da22508d22c800000000029050689832b717e44762782f93d687ad7b07962bdaa67c110000\
                    0000001067380048c57c7271cb35853727c8bc4d760f2992f1667e00000000001dcd65004ee6def9\
                    919aa3dcae4bc6b557a42cf5c8ac3d5e00000000003f346b80612430db2d8b1ba50a740e96752b45\
                    b3ae51e19d00000000043bd901e783e190d47c8ebc963b056b9f5b1a9426e7cac03e00000000009c\
                    765240968d7ba6af740e27d3685854dc89985220dcd4c4000000000029b92700b98e4a742d09c063\
                    df85932dc56fa2112f92b42400000000001dcd65007c7450a286b85edebad3e890acca0d453de101\
                    7a01000000517da02c00fda768991255ad66cfe4e5d0d40a81405e1c38b5000000010001fa400000\
                    0028bed01600000000517da02c000029bf7f69711e85f15f1f6b003b648ab4b0a987000000000252\
                    42208024c1d5d624adc13b41ff91e6e72bb78a9493d39600000000005b21a51029c3404d167962b2\
                    ba4ef630872fe3333708c960000000000051131b6f4f9b9d632147d07fa24c6b23d9b519d640a668\
                    4a0000000002098a678062c08fbccd26bd23748b7de1d1e71859a6718a390000000000579e6b8069\
                    479c2c05c9c961c16c0e3dbc13168744f9c89300000000015faf679a6d0e67cf08a1879673f9412b\
                    f124d5668b42742d00000000000904229b7e20eea835339666dc4011822f0245f411f73fd7000000\
                    00011cf22ac08e8dcd3612adc818de33c3100a7c506f9ecc6c4a0000000004f35d711bdc24ac5538\
                    b857bab8f2b30635b4b36d799e89d70100000056e08993609b3e6fd99faa212eb6f4aae756d1d4ce\
                    1997bab7000000010001fa400000002b7044c9b000000056e0899360a0208f5181418f8f558d0097\
                    19410cdda0e1864f0000000000127a3980a259073902e42cb437795ad32fbb9edd6a7f6dc6000000\
                    000132caf774a80d8f8e47c1e70774561924e5783439f728ad43000000000789b34bc0b125b63bfa\
                    cd180ef19fecfdd69c1fbab1036fca0000000000684ee180e27e14eab6c0b2ade569d1828b59b799\
                    fab923140000000002098a67800486c2fc0580ce5a3f81a4b2498a1521517a2d8800000000a3482a\
                    2fc80ab2f12da31cc1f08b741c98df78ee4f3e3c5a1a0000000003b5744fdb1892477500eafb8192\
                    dbdeaf60cf5d44ce0395cf000000000553871c4033315765403d336b73c663fc20c0836593b44450\
                    00000000002625a000358e06cb7091d21d9cb90f3053746f3be510dd3800000000001dcd65083991\
                    d4c0be3bf7c99809185ae0476e5c3fffc2f40000000003b4e7ec003b756984782ef0602798591d58\
                    38ec22f76bc73e000000000bd199e56f4367e8565ec3133d783ea68fbd89e36426323b1300000000\
                    02098a678043e7b92cc29a56b67bb494ede3f270493f98b2c5000000000217f4f4f75644a3f0e1e6\
                    be6aa2b592f59ac5e8255f56f2390000000001de042af3579dfd0a2d9e5ca57322b6c7827b2c9d85\
                    21c3450000000005d21dba0064feb3e250db1d929eaf63c6a4f7a7ca7dc9659a0000000000535241\
                    3d6dfb7a9e904bfa5726f918b7d559fdf3005a753700000000009c7652407c880f8fa504ba6f3c7c\
                    e2b37acf71e6eb264e4000000000003209a49d8536b84fab2786edd9b5689d32eb8bc0dc164aa400\
                    0000000030fae880a7301b3353e7ca96d6f5134245e27006bcecaf2d0000000001dd7fd387b6d905\
                    d6b5b685d54aabc31e8ed81ab02551fb06000000000224723b92bb5c650329bb9d94e3bd458834b4\
                    03406c0ad9b100000000041314cf00b9f566a93b98f8e201d58c95437268c918f843940100000019\
                    f75c3e40caf2525d62116d1ffd9f26166fb273893b11d61f000000010001fa400000000cfbae1f20\
                    00000019f75c3e40d4257575632f5bee84f1da96423ec525a2e87c2c000000000af09c9bd1d93bdc\
                    eb281855203f154d559863bb6fa3fa5f52000000000014f1af0bf0495fc75825da6d19538cdca879\
                    c7529edf05f10000000000d09dc300008b1c09dc3b5a9f3522e3c9ff38be5c23b189540000000000\
                    144cb3660d96906ca547926eec1378c72173d73afff6f67e00000000061c9f36800f2e7d90ba023c\
                    c4bbc466042b5448b8ae877f3300000000006a1fd4f115f7c56604c5962b299059426fb4ab9b6602\
                    60c60000000001822e00d3207a1538f1a080b6a004eb3376cb9f52a3eac6730000000000be420e00\
                    3704f6e8bf1336880f46880bb96e6244f9f3782600000000000dc755cf4a88aaad038f9b8248865c\
                    4b9249efc554960e16000000264eb62cb3ef64af75c3aac5b3b428d2ba1f04e194d35660a5820000\
                    00000826299e00657d865cce807ac823da15a4fb1df30aef83eb1d000000000188cac1696e291104\
                    e658f58073cb8c92137bc38d5938bdf9000000000026c962098d167435b1de223f4933532fceafa0\
                    f24601a67c00000000029b4a8fba8f49e79e8286ff58cbc114416c0df407880da26c000000000389\
                    b2fa70a95e35672d791c54e4fcfdae52888bd2e3eace57000000000005f14d20dca926ea86efa699\
                    1fa7eac8466b848b4f5182a5010000000813eae6e0afcc373acd784edf78aa520663f7113d40e8d7\
                    e6000000010001fa400000000409f573700000000813eae6e0b45f87391d7dbefc68d48e2526c87a\
                    84f2648a9a0000000000908a9040d53c0ccd94640028082d3a650beb44361b3a9f9e000000000543\
                    4dab00d6ac3b190b94d229022273b423caebc076aa332f000000000000989680deb98e363faa545b\
                    777ca5b387af45ea0762a98900000000003b9aca00ee39e4430373bc575126885b5cf7185b5ee059\
                    60000000001da8b11152f37c6e6435515bce43a0792822c6275a4af769f7000000000001312d0012\
                    eb384849f3b717a0cc346faca0e27bee3b753800000000041314cf0019cbb770041f4eb52bf64997\
                    e331485401a65cc8000000000066851e002870c479941364fd7ef5eb19f05a9ba6c8770f69000000\
                    00006ba2524039d49a14c355d3286570192ee088460bfdd8ca950000000000664b83e853c229989d\
                    ba30dc01a479ec5aef141130a5659200000000121ced096e5c33ec026903fff2c427d5e35a664e04\
                    8325084100000000072536d16297fe0b030b0e3bb9e286c5f947cfbb4d28973da500000000007247\
                    ad20ac794620c1696807b7b34f1702041d75326b36520000000009fe6d6b74e2d80b16017a422208\
                    ec2de85bff22b77185c3330000000001941e9700effb3bd733f048a7055d53a3f9529cbe4f60b4cb\
                    00000000003709f740faa1a1699c69136cc98fd31062430783df24a28f000000000005f5e1002203\
                    8946c93a95cb5367a0d4b17d6f156617466c0000000000009896802c89c00d0b2470ef6d4ecf9185\
                    6b2bb77b1b969a000000000138eca48036a9db1273b4e43126a1ee65cd4249fdf3780fe300000000\
                    013e49ef00566bc15dc9de58fb751fe155d6622dcf8c7283e000000000003c4d6d10704bf66340d3\
                    d884ec6af406e354692c49bcee7800000000087f7b10b1746411006cad075ce6f8c66ac42048ef81\
                    c6612b0000000000cff588988589ac765cb763bb8c5369a6e72f146d80d441220000000000009896\
                    809a1e7ecdc3167886e1380373fccac35e435373b40000000000684ee180a33859e3aa51f2535921\
                    e8c0ff553e6b3dd11bfc000000000079db2479aa543f0b248a0d70606dc2083bd16ee4478fafe500\
                    000000006771a0e0c98f13531773daae71eae49f19f9b504eb3d06ff0000000005283978f3e488ee\
                    9e2ff633565227e064fb2b8c55080b776c00000000015ca95c98f5e984c0e174bd86322b1c85d7ae\
                    9218f9b7f5bc00000000003fc6e780f763a36871e53e4a853aa7a86bed241df1d8bb4c0000000169\
                    09f21ec6f8b0e4e8512c87e882a1d00a5979022789d77f3c00000000cbba106e000cc92086cb2205\
                    a83a50cab9853abc367ef1ff3800000000002aebdaa014816f205bfa0aef472578eac86d280f91d4\
                    070c00000000027bfb4780038bb07accf595e7f4616ab62405eeba90a8abef0100000011601c2020\
                    26149b477ffc32f5f17f15985b9df73976fc6145000000010001fa4000000008b00e101000000011\
                    601c20202c83bd300ecdc03c9dc853233736d6cb80f56eee00000000083aa6bbb0378aca95f338af\
                    41314449be8e8f4ea0a8e0a52d00000000041314cf004094dc0c8376d6514f9710894d637228355b\
                    ee510000000000d09dc300463dd84365d39aebb8eb9fae59182a564458bbfb00000000002cb41780\
                    4fa92faa9b2bc6640d30d44d479e75bdab31e82d0000000038d880faa07286cd2bfaad56a96b0899\
                    2fa301b3d6244a160b00000000061ad57300734e550f02f1434c8acde94fee5156c18e73e2f10000\
                    0000035e360360da79d2a81404e24f56c58cad2d5e10ff7e403d3d01000001319718a500dd468814\
                    ba5e31e191ca6c4621053c75f6ce7a7d000000010001fa4000000098cb8c5280000001319718a500\
                    ddad4970dcb89dbb92f2832603da0299ceba334d0000000003e56727c315cb035ec98c9308d61b81\
                    f083b0dd2658ab20e2000000000a2fb40580208a18a517690fdaecb6a15986840d60534420680000\
                    000002716e52302edf89f377b2be233ee52d99616b2a2a72b8e9fc000000000485d1fa4035d6168b\
                    a468d2647dc12708cc05ede8d6ca9f8600000000054319335a3f5c042618dc7f909304c569252d32\
                    4f4eda6f5200000000174876e80043c27686a45242b4676a921cd7c035aa135cc7aa00000000032b\
                    0c296853a87f9221c494b5a51afc9e2183016b0753deab00000000041314cf005bdf1aeae384a73f\
                    5df517811f9bddf4978188d00000000001a13b8600651dbffe7f1a91cba9b543a4c03e04a9a55d74\
                    680000000002da282a80745e3112286c20b1b2e77dab44f12f2ca39df3b40000000002540be400bb\
                    5049c1620eec25285e91ad1273713eb892def90100000000f49f98a08a417d7cceb41b0084356247\
                    d2bc0cc3f7fb3464000000010001fa40000000007a4fcc5000000000f49f98a0922c87ce587cee6e\
                    2fe267d96a2694087c9531f0000000000169f2ff2099d4f59149e927309ea7b34511732021d581e0\
                    080000000000d09dc300b3e86242d194ff7e8377a7d9c7e0b1eb326e35e80000000004cda8e9bbba\
                    dbb3d93160e90612089080676cbf3357094ee50000000004f689cb0fc764239f77e8079604bc29ca\
                    bf24e0a703fc35ed000000000008647000f496da3417067b60f57f2785e263034f11a73931000000\
                    0045d964b80014c804c4d80bb639b32f01d4d2db5a4fb29cad6e00000000054c01738018aac26dbe\
                    5c54d6fe502ffdc0b2e4fd19df9dde000000003887b48c684ef39a468c406939e5dfdd3336c10f2d\
                    69b36232000000000006307900535edc85d7b5d0eb0c2d433bd7f42b8ccbd0e04700000000006553\
                    f1005ed2d51b9cb3bed909766eccf13c1528245ca49a0000000000684ee1806c25505e92c2a7bc30\
                    a6186e7d34bca8df424de50000000000009896807ba8904bf4fd49bcbcccca76820cefcb9f37d31c\
                    0000000004a817c8008ba80d6bbf3f7da5fa25c5a3b7733b27209883a40000000002da282a809f39\
                    d2e6d3d378cce91cbceefa26dc1322fc99fc000000000097595b85d3c6cf68bb8d7b86dcaf807efa\
                    c3e58baeebdc960000000000d09dc300e3f391e961efe25b7299383f01a0f07d8be983c700000000\
                    0f177bd1a0eb4426b6fb46957b3a2f2151e6ae0777cd23925f0000000033e92750c0f14220176d02\
                    916c3a3770d85f3ead3f2469330d00000000105558c706fbb60f73c5e1e0e7d1e6413f8eea28cb62\
                    a449690000000000b784693203274715eeb0dafcc92d49b3d5a242b95d2ef7b00000000001583781\
                    c0097ee4758bc78000f27fdfbf91d1e70b6223d493000000000549b036fa1b3cabcf7700756534fa\
                    7ada8cb92a589aae1a010000000000684ee180187f6a5baa47ef0989c761ea95819aed876a01cc01\
                    00000022ecb25c002957847390b1f556f3763c15416e8c6eccd7f806000000010001fa4000000011\
                    76592e0000000022ecb25c0054c67d869528f5316f8fe131cde4760353abe0860000000007aafbb6\
                    53639b82c4233b77286a1f56bfc2366bafbc81fbde0000000002d3011c8082754f448f81029a7c31\
                    f7927974416b6058eeaa00000000000e4e1c008722ce50ae2b4c8263ca8644a0905633fe7d290100\
                    000000138eca480089445dda013c7e61d08f51525cc7c10a0d84822100000000070cc9f1358b9aa1\
                    b77a7f43ac2f4d944f74f94cb216c2bd8400000000003cf85a5e36e7aa58567373e516e4909a013c\
                    f35adf17df8301000000037e11d6009103403910061e89416a2b09967b8085513d6a990000000100\
                    01fa4000000001bf08eb00000000037e11d60097917c6c5fee055df152ce3da0f80ff12c5c5d6300\
                    00000001487b2fc09c973d17f24b1fc1d66a8a8ab4a81d7956548c1d0000000002b71d9e6cdcd492\
                    9eded73064f668383c066639404bb37ee100000000002cd11560dd22889ef7e7e8109b7d02c1547a\
                    2c175781ffbc000000000e054c0318e03c8ed2e789f9edae8fcf3590633bb2b0a565f70000000009\
                    2928ded0fcb1ec7f993f561a04efba683b1651528e3b8cac0000000004d0917146fe16b1685673c1\
                    6e23cd7d3cc37dc57748ce025a0000000012eae09c800dea1e2c69ac5406e3bd96a7b9494b27467e\
                    e3dc0000000000c7dab8402e37d28d68c051ed05fbc77f90119eccc1de30a400000000003d268159\
                    5195f7a76a62aef990a6c5f57471461ebb507afd00000000104c533c00711877e8f5698feb433e08\
                    fe1308dcc30214ae6100000000008f0d18008620edb7e37adf4d2d009b1d899c8e2c3166f6210000\
                    000003810cc680942fbc821fc838d8b4c709f96f2cbad0778dcfb70000000000677bed98bab41249\
                    fd4f5d93c6adc4b9c90fb999642f50460000000000cddf79d8e90d2910d5470795406e208a0e2382\
                    e00727e57500000000072cebdc900063d97bb3effdfede3531aba9fd08810364658a000000000091\
                    b097a21270243c0180f90be55d108d1931f51671dd57e900000000005de598f529ddc433e00a7ff1\
                    b5f3c7293131a2e037cbd957000000000023c346002dcd2c604fcfe9693163210c06cfce950ca286\
                    67000000000002d21d3c3405b896e66938e31ea7b4f7f2401a9ec10a78750000000001931228b649\
                    aa45599fb5491b9fbd7b76f21214291efa00ba0000000001df2cdcc77067e83def38f58d770010a4\
                    2ae1a5af56b16c2f0000000064eac7e3a577f8d8f58e84b2c67da226280951fd003f05b7eb000000\
                    0000b2d05e007cee418b519c1ad7082a8fe3e11a7e13ad93c3be000000000010a5266e92070970cd\
                    4e5853132baa7d8a0ce4868824a28f0000000000684ee1809d241b99e72e5e2d79e78203189bf40b\
                    3b7952b9000000001256a11ab804b9962dee342eb7c04b34627ab2a0e42df8dea70000000000c708\
                    c7970c440093a95657a1681212fa5472c7fc2c55a1d600000000004cb4965318470511b9d7202748\
                    fdca49dde600534b78cb42000000000027a318401f421926804e064ea68891eccb439d31688864d5\
                    00000000008b874ca0222ce938995fee24865d53a28bbc0debc2ab70ef0000000001ac9fe80d2d2d\
                    48f1972d3353c2a4469d6bf0b96c3f4d848d0000000000d09dc300442b49c889d9ca76100ce0156f\
                    2a69a8c4b1ffaf0000000001dcd65000459a88c660f2a04b819a1a5e318c8abb41c955f200000000\
                    0059f5933a481c665ac14b52443ce2c419283c78c7b6b6444000000000059e4294804999a6eb6a2e\
                    6cfa88045ecb6ed4d5f6e32ec8a1000000000271d949004c796111f3cff4330d4a6c52964dac7482\
                    2e2a2c00000000015e2b21e35cefeff1347f24fa7c87d8e898cdbfa12937971f000000000066be56\
                    70647499b150702e872d1990fa5d28ef64efe5b97a0000000000009896805ac5d2d01f8ab891ae82\
                    81262b406863c21bd3de010000000cec0ecb00b80a400d569c1e9a164af334e53ddee55f78717000\
                    0000010001fa4000000006760765800000000cec0ecb00bbb805b0bd9762607a15f231bd7291f6ea\
                    e806930000000000382c1ceabd1cb7963577c51072b1a31693d674e282a3d4330100000022ecb25c\
                    00d66843fb6ca5d7f9e469046f2bf90d691a1a7ad4000000010001fa400000001176592e00000000\
                    22ecb25c00df19f1de808813f2a45558fd48e35a78cad1179b0000000000bec6ce6026d1ba1af297\
                    15657f459f4fab25084c868010b000000000023026d56e3eab0d0cc412705a42409ebf5b7feb6589\
                    a2fa720000000003941fa1905d6efa1ba31887de42c2fce28035c300f3a5bffe00000000000a6e49\
                    c05fa4c476311b91b28f3d21452384d9248bd415450000000002d934daee6384cd77f1ab6e23dedc\
                    e9b45cfaf9f8a1c6ded600000000002b6c53536a664f46a6a491a7e4876c1a9f8937c3d1c9108a00\
                    000000196bdbb2067e9f9bb12e0080536813062d030d8db6882c67c200000000001efe92008addc9\
                    28d4f72f012ce59a333515e6027972ebe00000000005e40933dda3a3747d33c9f273ff3ef5eafd21\
                    92b8348259d70000000000e67289c6a86d9afb40bf333c2ccd0aed804f396ab23dbeb50000000000\
                    d09dc300bcab74d57bd7b5c33fa1f6cd18d72235b8b7edd80000000000d09dc300d8cf783a965d0f\
                    8d6b36dd50be72748e65f4d91100000000000d57080cf7fdf7b8743b3149aac39e7b315c1458c6d9\
                    610100000000192739796fff36f6c690d24b8abaa57484ef3255d2c3582e63000000007ab13de879\
                    ff6af7a2e5dc70105f70f408a8eb59a30beba88800000000158a8a095a06baf3a6174dbf36fb017b\
                    b69fa0faf3fe020f59000000000067c95dd019a980683646391210514144141308d73e9763250000\
                    00000067292630351880c1e5cc48d65bf84e65f92688197b5f51110000000000d09dc30041f55d4b\
                    8f2386da3fd9ca5d4c2546443675e62f000000000000a7d8c0424194f927f7e09701b4decaf86e21\
                    67b4447a2b0000000000a1ad772056f65d98c9c570d01903f38052d90e037149438c00000000006d\
                    86066064f4e90f615af5ad44756a18026c2d5123f646aa0000000005b114f72f9379fe8ad815b1e7\
                    d2d2656d4b2d70c37298e99d0000000000c861d62c9f442dd2d5984a7b4a3b9e26a08edea5214eeb\
                    ae0000000006bfd4dc10abe298ae9d97ffce3dea2292269bb73bd23467890000000000c069209cae\
                    805a939846f35a3a7819313b99fc08bb612f4b0000000002a600b9c0b5635d09cb4aa4e53e13c8a8\
                    3a3cbe78770b2ef0000000000a2fb40580dd01a614aa5a13128850d70f9e3011897fab6ee8000000\
                    0000c62f7940e69f607c7476b2ad550afbd073461445f8c54274000000000022368b80157c491e02\
                    d205c7f3424d8a9b26c2a1c0ac863400000000025be8fc6031fdc0eb3610c71ddccc7c3e70dcc07d\
                    fe5e730c000000000c8c1824803bf0136b71def36633c16282d353b21a23cb04e8000000000c0c59\
                    1c2e449b7c6c3f42f66b9590f3ec3ed7d589a193633100000000041314cf0057df59ca6552f02e71\
                    6fc268b60cf96bb645a3b9000000000066bef0f85a42a905eb4b92852a08bf0a9a0d909f9b3e497e\
                    0000000000b2d05e005d28045a2ed817dea68a2bfe9fcaef0bf85d382f000000000251afa4807d5b\
                    7f0bffa087122bf45bdd721c4234bde79deb0000000001caaae4a1619feebb031c22928943cfef49\
                    a2d43d122c6f2c01000001977420dc007dc5ae35322d2020e9ef8967781b967a99a95bb800000001\
                    0003f48000000043e8b024ab000001977420dc00ab409b4c4f940f1ddee823d14aae442738b20c1d\
                    000000001d837fccbaae73f605dd7829ec34da93000b294aff06cb7b1000000000012bdb2730b67e\
                    29d613bbf23c22aec19083a1f7d97e4ac0e7000000000f7b90da00c08186503c5968e4f96c3177c3\
                    73084fd1a079ff000000000082bb171bc952e94a189f0d8b9752e44c1916cdade1a3e59700000000\
                    02098a6780d6ede141274feaaf7b59fbde74ecb953c71fd688000000001c0ddebd2eea62f93ddc2d\
                    5ab754fee4cdcc8a638336ecd1e90000000001097fc119f29bce7a79c869d1261ed2614b8c2f8827\
                    c340d70000000006f7ab4340f30cfe8c502769c165440db0cc7c5f7bc4b13cae000000000044babc\
                    2a1c7b254afde504141efc1278dd7e1bbae63177ce000000000035e05a7a25085ad333036c88fef7\
                    66b0bf845b8237c09d720000000000d145fd682b391bb997557ee5bcdd806f1498b4cba88defb800\
                    00000001348935de323f39b08a69e07dca65108bc46dedc041965e8d0000000001c8d55875362ea3\
                    1baf01030f683309ae91a72f544192436b0000000000d4c7da25600fe8de60c12045941a8f8e29df\
                    809d1a5945fc00000000019268a9a070f2d8f498f29022b45cd98db8cde1a331bec2140000000000\
                    08fb877f8f51909285939306d53f5dd7a010cfc8714f13f0000000000171d5135aa9989821685466\
                    262e6c1fbc09bb1c1c2d5cb94200000000846c244780c3d85e9db6b2ea524d741f768f1f9d387d62\
                    23660000000000486c2d39da169db24ebe463b2f3f399d2506a281e26490410000000000d963aaf6\
                    e931e73605b8e015aafa46c059b4c938a5565e9000000000003296e5b0ec35402e0ca22b1ea85298\
                    eee177bc49e90ad1060000000002bc75d8e0f0ff58e6ad7336ba2769a0e3d66114f4b5363c0f0000\
                    0000005089ad00093e43a8f5ac3c5b1afa63890227456a6472c6c40000000000af3cd700384f5f4b\
                    dda58b7ee33e141e556545b5e8f384ec00000000002602e1c348b018f3532d7e99b468824458741e\
                    ea99e14574000000000bfd60e96d5cfaa0f9b07c01539f475623e41ff970c6d688210000000002ee\
                    6c278077d22c2b303d45abd277640f47439434481df51a000000000015e79ae095616f96f7fbaa08\
                    5a2b9b389051efcfbdbaedd900000000023db1d840a2aafeecf08839f7ba15affe137b141ecd76ef\
                    c0000000000254a47a80af10f4e2f543e73871702f4814649e5f82ea4062000000000074d33a00b0\
                    448459e5bbb18941381013cf4a15dda8d0d6070000000005d9992f74bc3d5f818bb289e2d0243fa9\
                    57bc1c1926f5f623000000003279b0e180cd624cb6661f64b7bb204a927a9d1072bb9c24f9000000\
                    0000826299e0049d58ed26a452e6e5dd6d62e40451544bb504900000000000684ee180459407b276\
                    3cbf4b77b303588e19a7f0f4f3601c000000000014dc9380567ffe755542bb1ecb47be61d03ec844\
                    1e889c6b0000000005602e5024582ae312177dca3adae38ce582a4988bed089219000000000067a6\
                    a7188d09f5b96d280c1dab30ef6176838072f5e835b70000000000d09dc300a3c6b754ba760c2ea0\
                    5f5787e8241f3ed760a36b00000000000e9a6740a754cd3e92f61d24f487a63d4915aecd09596d4d\
                    000000000342770c00aaf9d78c7d2ac10e958ce543b9591dae5752d2e200000000043ccdf600ab6d\
                    fee4bbe68b983ab47c4b4588d8e360692eeb000000000059c8cc07ca3afb38685f68757670c1b29b\
                    84c95e071d531f0000000000217ab022db1be5f6bbaeb6ec78736a750b3d68367ff5f82b00000000\
                    0000989680de0086b188032758faf4dddec921e3f16bcd798a00000000020ff40a4af1948fe53bb6\
                    adcf133b60f1eda8691b1e5c832d01000001977420dc00e336165bc6a094ba3a02d9f330f3a1815a\
                    499a14000000010003f48000000043e8b024ab000001977420dc006152c081a3c42784b9f8ec51f3\
                    120a9512ca12e20100000015b822c680f0d90cfb9ac370b445cc4b3b10632d6e2cbd89d900000001\
                    0001fa400000000adc11634000000015b822c680fd65e5744fe4f66dc4d504e0fb988d3dd3afc2b0\
                    000000000012c684c0041e573a68aaf6f63d62277a93470a2db546c42c000000000ac4b6fe8010aa\
                    81a18582cfea054e997b5f65cf4e1a77983f00000000012a12c3401a255a972b52f4e990bf60ef8b\
                    d45edfb2c5d88300000000025b278050226d357f4a31310827c8160c6dc8bdc6a1515bce00000000\
                    00684ee1802fe1c2789a0c8c9c9618a05246d6f59a3c9919530000000007c632f7803531e14764ae\
                    2efe80c5c31556174538f4a5d4920000000022f89e1e00460cb613327ecfed969f25b9424e3864eb\
                    9f4e64000000000068029640473d8a57c741396902b3c7454b27bdd0d6a134ce0000000000209b2b\
                    0063a34a9fbcef1e62eda4711a62f2c839e89d4b87000000000000989680645ae642aafd3e62787e\
                    75b8bff980ba9513ae4a00000000019c2518c56a30ee0050b9c3f7e29acbe615bcea9498c9f52000\
                    00000000359efc6870b58b43e2a44d00826dbad5d543c1d0f41e0c81000000000267d5ef10787612\
                    82c37682b801473ebfc649a84053493fa00000000003810cc68079d202f62c8d05c89f13a461ffde\
                    25f0c705ff590000000008d0c2357c8b0648f256a086dddccbf5312aceda8deb811d2b0000000001\
                    500d0d80b7816b642fcfc7fa3bd0403c661eca855710faca00000000041314cf00c7f9cd94923b41\
                    b3ad4f2741004deebce12a05ea00000000003e95ba80d7d340f7eb7b65caee23abb154f203f3e924\
                    6e43000000000191836069e4b1114a86ab74616ea9b7b825ce15931bf1e33b000000000039d10680\
                    f4fab2abe4aa1e8c7da7f82104f8514d48a95f440000000000684ee1800fd4b3eab9ba7252380db3\
                    57951659712fff5f0500000000001296bf42259178933ad25f845a032f10204cfc1d55a9b88f0000\
                    000000579e6b80299078dee7c2dc448f1c6e36b5041a0b9ef674c70000000000cbb6b9f03fdd114f\
                    ddb31693c1c81e839f3fd7299a63f0160000000000b6bc45226183604cacd1834606a771ab3700ee\
                    9804fa335a000000000009e8c610680777a5cde5cfe0130dca0c73c8f0634a52f289000000000068\
                    4ee1806f494e6b2a717fc4ce90faca520e75abaefaa49500000000000bebc20079de4495619e83ed\
                    63607d71dd65bda41e88508500000000007a3fc6c09a77f0787f78fcb35b557320cb8e67fa8021b7\
                    2300000000000ce90dc0a5507bd6c7990a7cbd7d62b34baab39ed0af7537000000000002faf080aa\
                    3b23342a0cbde043ac0c4f3729984881ba6a7d00000000006bf57b50d5c19ad331a048f00305ca80\
                    4cd2dd2e4226294400000000012bdb2730eb2426afa3ed909105bd5a29050d99e646ac42e9000000\
                    000513493000ab3dc5715bfb300493cac91e99f9f9b239c6809e0100000011601c2020ec74064fd5\
                    2b987aa391c8f112d5b5fbfd13e5a3000000010001fa4000000008b00e101000000011601c2020fb\
                    6653c85ca496cd0ead67549ba43b01fa192b0000000000000cd47450956c10a45f93a6f1551d2d43\
                    36ca7b7aef1713db01000000517da02c00466e5f891b5693eee6b7514cd0d4b41b1c9c8161000000\
                    010001fa4000000028bed01600000000517da02c0056468b3666a3794f544f8b480d73065c8514d5\
                    790000000001794c22805d45e7c7f599f2fbfae955754b420c9aecdd89e400000000001f4add4068\
                    812af6b461b8fe24b10458125c5f6a64eeb2d30000000004b2d1cdcc6aad52f4fd213b6b56724329\
                    efd2fca167becbe500000000005d81fd0077931b44c96c6abe8a5bb92be70368d91b7c9abf000000\
                    0015436e5f2085ecf7b47e236ce13977858dd7d20ef26808cfc70000000000037502809d1d847d53\
                    b5391b92fc814860870b50146e6abc00000000042b091bbe9e2defc01f109f39258ec99ec7cf1f40\
                    d69fc897000000000ba43b7400ebcd43eee1bc3e65256db7693bf8545b6460a6f900000000016c09\
                    0de0ec06643251ebc320a967999091f5fd6d921d957c000000000052a0867af4da56a25f236be6a2\
                    72e02b864b9f59e04a1f2c000000000d722b1180fbc1f648a3a3397fe9e7ac927cbb75a0a5bf7ccf\
                    00000000006959e8e0fd3937027ff09c5c295d3ad9f1f9cf84a6322640000000000000a7d8c002e8\
                    105822f3b7039d3dae5afc34aedeb2b0186d0000000000131d7e6003e7bfd63b373b4ebfd6b2e078\
                    287cb55d1dd8d2000000000e66690220179f9df528170f8c191a9e1a8a09487f80fc972a01000000\
                    15a37707000e4efeceb2111428012ba546ee801c88cd874628000000010001fa400000000ad1bb83\
                    8000000015a377070012a84058cba3e5a057f20ee0ea9571a69b954cc800000000019781eb421523\
                    dce52fec5daf966e2a601437e889b5f7113f000000001732f1c59418475a184a480ee8a66e2612ce\
                    1290cef16c569300000000000098968019a96168b1ee3f698868bcc79ebaf81bb9bec7b400000000\
                    00a43fc76b289ce62b7c304d03373a119fad3170b6f12304d1000000000001bcb7282944d33c9584\
                    55cb5341a730564b2c7a493f55010000000006521a42452d63176728c9941684367c8df22269d436\
                    bd530c00000000010c388d0032eb8d34303feaa007ddab51d4eb34171f09b1950000000000be2198\
                    3c5fe88b77e889616aa0db5f83cb36bf0194ab85b4000000000342770c006228cdb46cc1e222543c\
                    1dc4eb2d1745b5819ea90000000000684ee1806357a04c1c2cc20c58148d52dfd177a160618a2a00\
                    00000000611f24cc7f53858eb74cd1bf7a2b4d874c51b20bceaf5fb5000000000bd4946f708807f2\
                    4e7c64452646bd628571395ff82bf1990800000000000a6e49c08ba9427c28734553483aa2ebe8d3\
                    8527f15d78b1000000007a3c704200912a686122e90db31d11d8b717f41585c3c810fc0000000002\
                    60432d03ac4e3afe4c75cb13e14f3d2c456c0c6678f89f4f0000000000d09dc300b23f2135e4efac\
                    ed6b4ed0ba673d6206c61ec7190000000017be2d0647d8a3647b318d415c7bf0f909c5dd31149584\
                    7c7b0000000001e069d700fa0fa85982c8fb0d5f16d8fbe9e343c7dddf5a4f0000000003d8a07c27\
                    d12e0f24ce0ba902afd72dd02d984798da887a14010000000013ddc120268ee9cb7a2a1b4fb10a39\
                    3163ebf1bb253dca94000000010001fa400000000009eee0900000000013ddc1202d9ed186b202ee\
                    438176b3709944b357bf92df2f000000000271d9490032af6e62c122cb08884575a65ff43ef239b2\
                    5e8a000000000061b1367c39bcb6acbc6eee0f0635e3dc5d5c82989fd611d700000000039abb75a2\
                    59c1ccc185ceb45ea42b0616f3e4341142c1ebd30000000000bc46f97f5bc0995dab943f099b3c7d\
                    3ffaaf326da41d6f4000000000016048e3b063c222d9777383d84a3e05eb26d1e5b102aa33d10100\
                    00000135f1b40070bb852483af3e2fc565b57a212e9578ec994a0a000000010001fa40000000009a\
                    f8da000000000135f1b400779d078c97e399ff3e52e066aaa7a4305faf11740000000000d09dc300\
                    8710d869ee52f5c896c6af48db885fbb3686e6ac0000000005600995df68093f5725070c6af0e581\
                    7c61c900fec0b7d702010000008bb2c97000a68fc5172d94b2bc17b7346699c526b8f7898d7a0000\
                    00010001fa4000000045d964b8000000008bb2c97000ad0b04757f1b11b2bcf0a3bd9d05855250ab\
                    e7600000000002098a6780d52204dfb45af47d9aee040d4dfd4ee659f59da100000000027863eff0\
                    e4cef8a2a16fac1ce26f58251cf526c2d1603da90000000000342770c0ffe48b4932ebf98ffed079\
                    2093c63ac9e392d78c00000000009b55c1c10a5803bee8ad8dea076e2b820b4050cc07c11b810000\
                    000000b2d069972b45c6ebe69bc210918305ca332663c8d5b601570000000000fe24ef9e2cda63f1\
                    dcd0c29ce7858c8b0b5b5992be728a930000000045d964b8005411e70d7794cf3858ab0b2ade8f56\
                    7601ec5dae000000000148d407f655cf7cad4d0381de9a4837330efcc2031fa23d3600000000061c\
                    9f36808ff0922102bdc7cc16c19d1bd8aeeb46c6d6d93c00000000018046075a95bbf0b6d3421ee5\
                    5e92a0f98d1eb50968db4a1b00000000005c221bd7b3163812dafb225481d1e10c1adefd3ececfee\
                    890000000005d21dba00e794b16f6c357c709eb1f7a6d761e69b34066e6300000000000c09eba9ee\
                    ea944ec3f39c781ae5cd867828f6b9451036bd000000000104c533c0162d306b7e2467925d01e567\
                    47ab35ca1f35ce2f00000000023bab1d9318c52ef7b5bdbb20c6f82d02e7e037e132d043b8000000\
                    0000810914511ae15fc742ccf87ca80ab63ac2e2192fd46b4e09000000001aa8eb73ec323a47584a\
                    5297d2bb2240499a6065c8a3e4f0e80000000002da282a8037b993a060b384f80ff7942fc3e0dd3d\
                    66f508de00000000012ea32a214fabbfdbec3e15ead1fe20abc21d649ad0788da300000000016685\
                    9dc05520534190a86d58bfe8bf23c248558c4ba2385e000000000950372c235eb163db0265319f11\
                    593250db03129b2f70129a0000000001e34b62bb6415bdbcdaaede689fcd67ac780e49361aed3553\
                    000000000192dfedb6683bbd5baeeddf706096174a8a76df6b40fbf33b000000000ba271b0809331\
                    39a1a575f7ef5266b94eb00be18b23c25c64000000000017b56aeec024711c863ddf881329360119\
                    c5863621f115940000000000ef409bc0c48cb67268a855b7a9246b2f20958339e1a0143a00000000\
                    0103d4e050fd22702dff32cdbb481176cc36e1b9ecacf6786e01000000adc114ad600fe2000f5edb\
                    bd459c78604ec9b923b56fc7993b000000010001fa4000000056e08a56b0000000adc114ad601f4b\
                    f8be2336827c8a874954d55d3638885d417d00000000000a5765fc25a11a4ff308fdf2b3b382462a\
                    5348bc928c9677000000000be0945de032396c0057675a55bbfb4f0a8d6b93f60365001d00000000\
                    001f26b9402a10b1a406eba1fe97b70e22be68e54bceb3ef8f0100000006e76bb78036a76acb4168\
                    7aa1ba4736368b755f7e997cf64d000000010001fa400000000373b5dbc000000006e76bb780473a\
                    25a3282f348c5f42876b073222123fbff7ec00000000017f0fa8e07449f076f287d58f07cf8e1933\
                    f51661c60e13b50000000005715c2b388294f45b6cba5f3ef5c844e4fcb3974530e9145100000000\
                    01a13b8600aaefb5e58b5010a19d1a365a8809e19aac82de7400000000012ae79b9cb26ca3b059df\
                    a1d451092034cee07c52c552230c000000000236e34a80d27b0ea419ff6f6862bc92188120a1e456\
                    d31f21000000001e1704cc33ddc43f752f8f596d2b3a5c6a15d8433f55fcaef9000000000d0a1be7\
                    7651e767ee66507e237e184cbee1a162e7d1884548010000002b46ee0e00f962d255fb6c8ba06b11\
                    92f7f73109f5df843ded000000010001fa4000000015a37707000000002b46ee0e0003d2fef7e8a6\
                    83125c75d355a6d42317dba073f100000000000ee2e814f095219727aed45415e36a2b5c19fb5029\
                    e89e7701000001977420dc001144fb0f59a1ef7da03ad76408fc7d57eab0e7e0000000010003f480\
                    00000043e8b024ab000001977420dc00279546cd3a5327d154ff8860b3e74c748e63efac00000000\
                    007444c5a4282787e57f03ad0993d88ef704d6f8b63d4a1dd30000000001248a23003bc7216922db\
                    157c5dd2194e797e665f68d71a9b0000000000d09dc3004076f4bdca02961be2fdd607780d941c04\
                    509b8e0000000000bd5cc160ed324bce36b78c66bd9aea0e001dbe4cf1eb8d050100000009590c8b\
                    6047488d3ec1c521b448495d27117cb60bffd0fdd2000000010001fa4000000004ac8645b0000000\
                    09590c8b604c6c13937e9941020c8404cd9bc299290792bc40000000000ae2846380609393725d1a\
                    32c6ef9e808b53762f1ef10805a0000000000138eca48062243ce8e78e7fc31e5e2d15e343c514d3\
                    ea36e20000000000684ee180644237da413f40f5416b301dbd21a31c91bcdc8b00000000061c9f36\
                    806ee6119e0b6585292ad34a71299316a482f49087000000000931c5df1e6fb3b4cd6052da5627ef\
                    8ef0090ec9790a432b1f00000000016d75832d3bca276c437c1c17a40bec0d2c51bdba4f5038ac01\
                    0000003c967fddc072a2a2f97cd7864f6cf471dace59981747cb13b1000000010001fa400000001e\
                    4b3feee00000003c967fddc07ffb764c47c1155d2b87be7afcd57da26d673cf00000000000fa0b59\
                    fb8412614ad8113a5d54c5ef33b07a6739d99b3d1300000000003d4bb822a8aa654e9506e47d291a\
                    219ba6327c6af6a18c4f00000000002de6cb20b311b33c9dfc5df0e8178487919402cd4523a28a00\
                    0000000188be036ec0119fe21e835399cd053571175bd995aa8ae8cb0000000009e79b6e9ac75491\
                    0da42fee00eb84210e68989ee3c05a2b8500000000019f71c2803fde33e53abe41b5c3c63c6cb641\
                    de639ee3ac48010000001a10296ce0080eaa9aa1fa175cee84a62b135b81c52113f2f70000000100\
                    01fa400000000d0814b6700000001a10296ce01017c3c6defe88a022660daf01f4aa124506bbc700\
                    000000054c716581212e0e716c52c0f54fdb7dd8bb0b73d5905a7c7d000000000051a88a80480def\
                    76f9209e5860ffbacf8826508140aa993100000000001f4add40593063f464fbd6744d78f620155f\
                    6185fee22363000000001921b9b1006d1ba9eacde5cd41e41dbaf395bc09caa27fd1140000000002\
                    087f60207b0cf6313e1a18ea8c80b7f2246d9da11115a39b00000000030836ab4489dcf37c39a593\
                    fbe93b7dad9be5bcee95f244b70000000000535c28fb8e8f7edadd6c8162372bdf69f8c2af59d774\
                    9fe7000000000b4725510fcd5fb59687fa21f09f82d2c392dd64965e4547a80000000001959bb2a0\
                    e5a394c4fbb99739e6125e5bc9c3708aea10880100000000010d0f8bed0a859fb3dce8a824a25151\
                    f389d0a72a53627f95000000000001fb5ad0149ef8aaf6d2755f6fed9a10ca123ec999952af70000\
                    00000171c5b670215e273f29f6f0ea0f048d60ec602614ad34d6120100000022ecb25c00285fa395\
                    b7bd1b1303f3e5e75d0142b3ca5f3935000000010001fa400000001176592e0000000022ecb25c00\
                    2d0f2ca4ffb222e0b45bf7cac566fdab8100e8a70000000000342770c040bd4f62f7cec9c2588ce7\
                    80909f9b22661ffd2b0000000000684ee180435b54f40121c8ffb4500fec9b0b1c72f361a6490000\
                    0000041314cf004937079ff49769a3a4a640609ead7c192f3d3d8200000004e5af8e5d484e976856\
                    c089b5e855d456bef4bb5562c5a2b366000000000b08a024f35b792852421a2c3cfd1bc2cb769bde\
                    df4c535c10000000000053efad21855a4b220698d4f6ab096b01f37ab4305307cc39000000000a58\
                    d1d416867b2ff89a0effcd9b327368ae80cd995be16aa40000000008bf7adbbf86dcfa56464d00d5\
                    c2d5c1c25359741a856f15f5000000000080befc008e0a8e2f68e8084d4bddd3ba8421428670f1af\
                    180000000000009896808e7013b7f44732cdc8ab2e5cb31e2737e02278b10000000001fc72d5cc91\
                    62443606feeb4b57705d90ef324e01139dbdcb000000000034dfcda9bd759e714618b5ad4251ca07\
                    3e35e134abf629bc0000000000d09dc30016f9abb72ba6141b3b15d5633114fa493382987d010000\
                    01319718a500c7f50beb51066e5def28e1f49d98feb72ae0a631000000010001fa4000000098cb8c\
                    5280000001319718a500c91adcbe5430bf9d9f5cd89311fd65151b6f076e00000000001ad2fba5d0\
                    6418511da16f8cb443f4b9999a36aec1d29aeb0000000000d9a4284fc09391125573874e379f1f0a\
                    9297ca74ae7fb5e2010000002b70458d000958e3590a269916569e7de9c631fcad6c6bd73b000000\
                    010001fa4000000015b822c6800000002b70458d0012a967c749eea600a432988e41948980ec03ec\
                    9e0000000002d21dddd615c10814895abbe27dae398311ac7a795e6fc9b5000000000171fb1e501a\
                    356b628c57af3bed89702d904864d8a0d4dc980000000001938a946033c0b260ecc2a8715cf7b707\
                    8d60585ac707a34f0000000000b1fc6f003e09b20202bca4a7e45f4768808c6d2c927dffc8000000\
                    0004a817c800476f02fca635de0dcbacac55502ea75e1dc8366200000000007b347a654809e9f0e8\
                    9772c6bd996794e0c31cc85e2854880000000001b618198068ee1c9a14a85ec2d960c5b50af97e47\
                    bc89d3d300000000001b7125806a3a1a17b47f869f1250a2c626508ddec86e84f200000000042422\
                    a740725631934f7c59bdb872d63f4ceb34672c84e36b000000000015c9c34774fc44d06c5360721c\
                    f1570e03c791780ef3159f0000000001aa5aa9a18800973446cabe423c6b327742f51da55b1f803e\
                    0000000000d09dc300e446d4a5ad6f7299138b6b2a3fd56f18ea23149c00000000007b8fb86ae909\
                    bab205ce9f86c26f3626f1fc91642bbecca1000000000001312d0004d2e3a7a383c25ea344bd112e\
                    4d48cc284b833d000000000084914e0b121a9af5c41e1921cab2319ee796a869571671d200000000\
                    1ba66d9c8b7412c4ce16bc1fcbf96320f04f9355a6a04e4b7f00000000009c765240e842a94923e0\
                    f2f20adb087f524f81aeb105579b01000001977420dc0097489276ba44fea671be53943e6d0712fc\
                    a587de000000010003f48000000043e8b024ab000001977420dc00a01fbb7a4ad8b3153e0d7eba04\
                    5d3f7e0c09475400000000007f8dcf00bd0c0a74a17a1382e6960e27f0d95ce545ca07e700000000\
                    040d1eee00c89656f5ed3e4ac13f1013c6da5fc36472b38c3c000000000295b24077cc606a4bb273\
                    e49a64603d037f083d97c22fdd130000000007d6f5e69cfa5b19d0a7b149aad96315bb432e5e157b\
                    2bb85b00000000005f10bf510039751a833f4641f4cfa4c42d1bd54adafedbe70000000000684ee1\
                    8034d6aa3f06452d1e221e0d25e34473ad411170f9000000000005f5e1003df3ee2c1b7f33478563\
                    900fe880379b46743e8d0000000024322636af4abc5029a3f4697c1641e6c7772b4023a423215b00\
                    000000003398ab2e5b760eb50f1d8740508f4e2fef0488fbb1e80bd600000000012ffbd3007e1d6b\
                    23752bbfd423b426f57d216917f8ae006500000000058ecc0c738ccff95c847f268899fff6abdf78\
                    ada7f1524f57000000000050a153b0964316d13b7365589448efa2dd44d4c5db9eac240000000001\
                    759f07e4b0b275dcca9a959a7e9c4c3ccc0e61344eff9ed300000000041314cf00d16c0ee40d4efe\
                    fc349182d5a57796f5ec31a4d6000000000095957500d3ba323877c19e76fe5ef6098b1e5bd9e913\
                    493f0000000001fc5e364058ec9228c514efaca08543ba0969b145f1ab039601000000114f91cfc0\
                    ebd4ec914becf85c41585d50a81dcc0f4bae4d3d000000010001fa4000000008a7c8e7e000000011\
                    4f91cfc0ef60558d333e77839026fa1b0574e1b0f1e4d0790000000029ea5e4814f3a531509d46df\
                    87e058c2762672a51366c3ce3b000000000014345918".into(),
            },
        );

        m
    };
}

pub fn get_network_info<'a>(network_id: NetworkId) -> Option<&'a NetworkInfo> { return NETWORK_MAP.get(&network_id); }
